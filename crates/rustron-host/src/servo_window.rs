//! Servo embedding: a visible winit window (`run_window`) and an offscreen runner
//! (`run_headless`). All Servo-specific code lives here. Loads of `app://app/...` are served
//! by `AppProtocol` through `WebViewDelegate::load_web_resource`.

use std::cell::{Cell, RefCell};
use std::future::{self, Future};
use std::path::PathBuf;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::Once;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context};
use http::header::{HeaderValue, CONTENT_TYPE};
use http::{HeaderMap, StatusCode};
use servo::protocol_handler::{
    DoneChannel, FetchContext, NetworkError, ProtocolHandler, ProtocolRegistry, Request, Response,
};
use servo::{DeviceIntRect, DeviceIntSize, DevicePoint, WebResourceResponse};
use servo::{
    EventLoopWaker, InputEvent, MouseButton, MouseButtonAction, MouseButtonEvent, MouseMoveEvent,
    RenderingContext, RgbaImage, Servo, ServoBuilder, SoftwareRenderingContext, WebResourceLoad,
    WebView, WebViewBuilder, WebViewDelegate, WheelDelta, WheelEvent, WheelMode,
    WindowRenderingContext,
};
use url::Url;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::{Window, WindowId};

use crate::app_protocol::{AppProtocol, SCHEME};

pub struct HostOptions {
    pub app_dir: PathBuf,
    pub url: Url,
    pub window_size: (u32, u32),
}

fn init_crypto() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // Another crate in the process may have installed a provider already; that is fine.
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

/// Registers `app` with Servo so the scheme is known, fetchable and treated as secure
/// (no mixed-content blocking). The actual bytes come from `AppDelegate::load_web_resource`,
/// which runs before scheme dispatch, so `load` below is only a safety net.
struct AppSchemeHandler;

impl ProtocolHandler for AppSchemeHandler {
    fn load<'a>(
        &'a self,
        _request: &'a mut Request,
        _done_chan: &mut DoneChannel,
        _context: &FetchContext,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        Box::pin(future::ready(Response::network_error(
            NetworkError::ResourceLoadError(
                "app:// load was not intercepted by the embedder".into(),
            ),
        )))
    }

    fn is_fetchable(&self) -> bool {
        true
    }

    fn is_secure(&self) -> bool {
        true
    }
}

fn protocol_registry() -> anyhow::Result<ProtocolRegistry> {
    let mut registry = ProtocolRegistry::default();
    registry
        .register(SCHEME, AppSchemeHandler)
        .map_err(|e| anyhow!("registering the {SCHEME} scheme failed: {e:?}"))?;
    Ok(registry)
}

/// Shared delegate: serves `app://`, records titles, and flags when a frame is ready.
struct AppDelegate {
    protocol: AppProtocol,
    titles: RefCell<Vec<String>>,
    frame_ready: Cell<bool>,
    on_frame_ready: Box<dyn Fn()>,
}

impl WebViewDelegate for AppDelegate {
    fn load_web_resource(&self, _webview: WebView, load: WebResourceLoad) {
        let url = load.request.url.as_url().clone();
        if url.scheme() != SCHEME {
            return; // dropped without intercept: Servo loads it normally
        }
        let response = self.protocol.respond(&url);
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(response.mime));
        let status =
            StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let reason = status.canonical_reason().unwrap_or("").as_bytes().to_vec();
        let mut intercepted = load.intercept(
            WebResourceResponse::new(url)
                .headers(headers)
                .status_code(status)
                .status_message(reason),
        );
        intercepted.send_body_data(response.body);
        intercepted.finish();
    }

    fn notify_page_title_changed(&self, _webview: WebView, title: Option<String>) {
        if let Some(title) = title {
            self.titles.borrow_mut().push(title);
        }
    }

    fn notify_new_frame_ready(&self, _webview: WebView) {
        self.frame_ready.set(true);
        (self.on_frame_ready)();
    }
}

impl AppDelegate {
    fn new(opts: &HostOptions, on_frame_ready: Box<dyn Fn()>) -> anyhow::Result<Rc<Self>> {
        let protocol = AppProtocol::new(&opts.app_dir)
            .with_context(|| format!("app dir {}", opts.app_dir.display()))?;
        Ok(Rc::new(AppDelegate {
            protocol,
            titles: RefCell::new(Vec::new()),
            frame_ready: Cell::new(false),
            on_frame_ready,
        }))
    }
}

// ---------------------------------------------------------------- headless

struct ChannelWaker(mpsc::Sender<()>);

impl EventLoopWaker for ChannelWaker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(ChannelWaker(self.0.clone()))
    }

    fn wake(&self) {
        let _ = self.0.send(());
    }
}

/// Renders offscreen (software GL, no display needed) and returns the first page title that
/// starts with `wait_for_title_prefix`, or an error after `timeout`.
pub fn run_headless(
    opts: HostOptions,
    wait_for_title_prefix: &str,
    timeout: Duration,
) -> anyhow::Result<String> {
    run_headless_capture(opts, wait_for_title_prefix, timeout).map(|(title, _)| title)
}

/// Like `run_headless`, but also returns what Servo painted: after the title appears it keeps
/// painting (up to 5s) until the frame has a non-white pixel, then reads it back.
pub fn run_headless_capture(
    opts: HostOptions,
    wait_for_title_prefix: &str,
    timeout: Duration,
) -> anyhow::Result<(String, RgbaImage)> {
    init_crypto();
    let (width, height) = opts.window_size;
    let context = Rc::new(
        SoftwareRenderingContext::new(PhysicalSize::new(width, height))
            .map_err(|e| anyhow!("creating the software rendering context failed: {e:?}"))?,
    );
    context
        .make_current()
        .map_err(|e| anyhow!("making the rendering context current failed: {e:?}"))?;

    let (tx, rx) = mpsc::channel();
    let servo = ServoBuilder::default()
        .event_loop_waker(Box::new(ChannelWaker(tx)))
        .protocol_registry(protocol_registry()?)
        .build();
    let delegate = AppDelegate::new(&opts, Box::new(|| {}))?;
    let webview = WebViewBuilder::new(&servo, context.clone())
        .url(opts.url.clone())
        .delegate(delegate.clone())
        .build();

    // Paints a frame and reads it back before presenting (present swaps the buffers).
    let paint = || {
        let _ = context.make_current();
        webview.paint();
        let rect = DeviceIntRect::from_size(DeviceIntSize::new(width as i32, height as i32));
        let image = context.read_to_image(rect);
        context.present();
        image
    };

    let deadline = Instant::now() + timeout;
    let mut seen = 0;
    let title = loop {
        servo.spin_event_loop();
        if delegate.frame_ready.replace(false) {
            paint();
        }
        let titles = delegate.titles.borrow();
        if let Some(found) = titles[seen..]
            .iter()
            .find(|t| t.starts_with(wait_for_title_prefix))
        {
            break found.clone();
        }
        seen = titles.len();
        drop(titles);

        let now = Instant::now();
        if now >= deadline {
            bail!(
                "timed out after {timeout:?} waiting for a title starting with {wait_for_title_prefix:?}"
            );
        }
        // Sleep until Servo wakes us, but never past the deadline.
        let _ = rx.recv_timeout((deadline - now).min(Duration::from_millis(50)));
    };

    let paint_deadline = Instant::now() + Duration::from_secs(5);
    let mut last = None;
    loop {
        servo.spin_event_loop();
        if delegate.frame_ready.replace(false) {
            last = paint().or(last);
        }
        let painted = last.as_ref().is_some_and(|img| {
            img.pixels()
                .any(|p| p.0[3] != 0 && p.0[..3] != [255, 255, 255])
        });
        if painted || Instant::now() >= paint_deadline {
            let image = last.ok_or_else(|| anyhow!("no frame was painted"))?;
            return Ok((title, image));
        }
        let _ = rx.recv_timeout(Duration::from_millis(20));
    }
}

// ---------------------------------------------------------------- window

#[derive(Debug)]
struct WakerEvent;

#[derive(Clone)]
struct WinitWaker(EventLoopProxy<WakerEvent>);

impl EventLoopWaker for WinitWaker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }

    fn wake(&self) {
        let _ = self.0.send_event(WakerEvent);
    }
}

/// On Linux/Wayland, `eglSwapBuffers` with swap interval 1 blocks until the compositor sends a
/// frame callback. That callback is dispatched by the same thread that is blocked in the swap
/// (the winit event loop), so the first presented frame freezes the window and the page never
/// finishes loading (observed with the NVIDIA EGL Wayland driver on KDE). Servo paints only
/// when a frame is ready, so swapping without vsync is fine. Best effort: failures are ignored.
#[cfg(target_os = "linux")]
fn disable_blocking_swap() {
    use std::ffi::c_void;
    type GetCurrentDisplay = unsafe extern "C" fn() -> *mut c_void;
    type SwapInterval = unsafe extern "C" fn(*mut c_void, i32) -> u32;
    // Safety: standard EGL entry points with their documented C signatures; the library
    // outlives the calls and EGL is already loaded by Servo's surfman.
    unsafe {
        let Ok(lib) = libloading::Library::new("libEGL.so.1") else {
            return;
        };
        let (Ok(get_display), Ok(swap_interval)) = (
            lib.get::<GetCurrentDisplay>(b"eglGetCurrentDisplay\0"),
            lib.get::<SwapInterval>(b"eglSwapInterval\0"),
        ) else {
            return;
        };
        let display = get_display();
        if !display.is_null() {
            swap_interval(display, 0);
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn disable_blocking_swap() {}

struct Running {
    // Must outlive the rendering context.
    _window: Rc<Window>,
    servo: Servo,
    context: Rc<WindowRenderingContext>,
    webview: WebView,
    cursor: DevicePoint,
    // Keeps the delegate alive for the lifetime of the webview.
    _delegate: Rc<AppDelegate>,
}

struct WindowApp {
    opts: HostOptions,
    waker: WinitWaker,
    state: Option<Running>,
    error: Option<anyhow::Error>,
}

impl WindowApp {
    fn start(&mut self, event_loop: &ActiveEventLoop) -> anyhow::Result<Running> {
        let (width, height) = self.opts.window_size;
        let window = Rc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Rustron")
                        .with_inner_size(LogicalSize::new(width, height)),
                )
                .context("creating the window failed")?,
        );
        let display = event_loop.display_handle().context("display handle")?;
        let handle = window.window_handle().context("window handle")?;
        let context = Rc::new(
            WindowRenderingContext::new(display, handle, window.inner_size())
                .map_err(|e| anyhow!("creating the window rendering context failed: {e:?}"))?,
        );
        context
            .make_current()
            .map_err(|e| anyhow!("making the rendering context current failed: {e:?}"))?;

        disable_blocking_swap();

        let servo = ServoBuilder::default()
            .event_loop_waker(Box::new(self.waker.clone()))
            .protocol_registry(protocol_registry()?)
            .build();
        let redraw_window = window.clone();
        let delegate =
            AppDelegate::new(&self.opts, Box::new(move || redraw_window.request_redraw()))?;
        let webview = WebViewBuilder::new(&servo, context.clone())
            .url(self.opts.url.clone())
            .hidpi_scale_factor(euclid::Scale::new(window.scale_factor() as f32))
            .delegate(delegate.clone())
            .build();
        Ok(Running {
            _window: window,
            servo,
            context,
            webview,
            cursor: DevicePoint::zero(),
            _delegate: delegate,
        })
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        self.error = Some(error);
        event_loop.exit();
    }
}

impl ApplicationHandler<WakerEvent> for WindowApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_none() {
            match self.start(event_loop) {
                Ok(running) => self.state = Some(running),
                Err(e) => self.fail(event_loop, e),
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: WakerEvent) {
        if let Some(state) = &self.state {
            state.servo.spin_event_loop();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        state.servo.spin_event_loop();
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                let _ = state.context.make_current();
                state.webview.paint();
                if std::env::var_os("RUSTRON_DEBUG_PAINT").is_some() {
                    let size = state.context.size();
                    let rect = DeviceIntRect::from_size(DeviceIntSize::new(
                        size.width as i32,
                        size.height as i32,
                    ));
                    let px = state
                        .context
                        .read_to_image(rect)
                        .map(|i| i.get_pixel(size.width / 2, size.height / 2).0);
                    eprintln!("rustron: painted {size:?}, centre pixel {px:?}");
                }
                state.context.present();
            }
            WindowEvent::Resized(size) => {
                state.context.resize(size);
                state.webview.resize(size)
            }
            WindowEvent::CursorMoved { position, .. } => {
                state.cursor = DevicePoint::new(position.x as f32, position.y as f32);
                state
                    .webview
                    .notify_input_event(InputEvent::MouseMove(MouseMoveEvent::new(
                        state.cursor.into(),
                    )));
            }
            WindowEvent::MouseInput {
                state: element_state,
                button,
                ..
            } => {
                let action = match element_state {
                    ElementState::Pressed => MouseButtonAction::Down,
                    ElementState::Released => MouseButtonAction::Up,
                };
                let button = match button {
                    winit::event::MouseButton::Left => MouseButton::Primary,
                    winit::event::MouseButton::Middle => MouseButton::Auxiliary,
                    winit::event::MouseButton::Right => MouseButton::Secondary,
                    winit::event::MouseButton::Back => MouseButton::Back,
                    winit::event::MouseButton::Forward => MouseButton::Forward,
                    winit::event::MouseButton::Other(n) => MouseButton::Other(n),
                };
                state
                    .webview
                    .notify_input_event(InputEvent::MouseButton(MouseButtonEvent::new(
                        action,
                        button,
                        state.cursor.into(),
                    )));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (x, y, mode) = match delta {
                    MouseScrollDelta::LineDelta(dx, dy) => {
                        ((dx * 76.0) as f64, (dy * 76.0) as f64, WheelMode::DeltaLine)
                    }
                    MouseScrollDelta::PixelDelta(p) => (p.x, p.y, WheelMode::DeltaPixel),
                };
                state
                    .webview
                    .notify_input_event(InputEvent::Wheel(WheelEvent::new(
                        WheelDelta { x, y, z: 0.0, mode },
                        state.cursor.into(),
                        servo::Modifiers::default(),
                    )));
            }
            _ => {}
        }
    }
}

/// Opens a visible window and returns `Ok(())` when it is closed normally.
pub fn run_window(opts: HostOptions) -> anyhow::Result<()> {
    init_crypto();
    let event_loop = EventLoop::<WakerEvent>::with_user_event()
        .build()
        .context("creating the event loop failed")?;
    let mut app = WindowApp {
        opts,
        waker: WinitWaker(event_loop.create_proxy()),
        state: None,
        error: None,
    };
    event_loop.run_app(&mut app).context("event loop failed")?;
    match app.error.take() {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
