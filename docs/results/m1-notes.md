# M1 notes

## Servo embedding (Task 2)

- Servo is pinned at git rev `75d20deef` (`Servo 0.8.0`). The workspace `Cargo.lock` was seeded from Servo's own `Cargo.lock` at that rev so the transitive dependency versions match what Servo builds with.
- `servo` is used with `default-features = false, features = ["bundled", "js_jit"]` (no multiprocess, webgl, webgpu, webxr, devtools, media). Windows adds `no-wgl` like servoshell does.
- `app://app/...` is served through `WebViewDelegate::load_web_resource` -> `WebResourceLoad::intercept`, backed by `AppProtocol::respond`. The request interceptor in `net` runs before scheme dispatch, so it sees every `app:` fetch, including subresources and `fetch()`.
- Secure scheme: Servo has a hook, `ProtocolHandler::is_secure()` (registered through `ServoBuilder::protocol_registry`). `app` is registered with `is_secure() = true` and `is_fetchable() = true`. At this rev `is_secure` only bypasses mixed-content checks, so `window.isSecureContext` and secure-context-only APIs (e.g. `crypto.subtle`, service workers) are not yet guaranteed to behave as on `https`. Not verified in M1; M3 owns `protocol.registerSchemesAsPrivileged` and should check this.
- Servo can be initialised once per process (its `Opts` are a process-wide global that panics on a second init). The headless tests therefore re-run each case in a child process of the same test binary.
- The window runner forwards mouse move/click/wheel and resize only. Keyboard input is not forwarded yet.
- Wayland present deadlock: with the NVIDIA EGL Wayland driver, `eglSwapBuffers` at swap interval 1 blocks until the compositor's frame callback, which is dispatched by the same thread (the winit loop), so the window froze after the first presented frame and the page never finished loading. `run_window` now calls `eglSwapInterval(display, 0)` after making the context current (Linux only, via `libloading`). Check a window run with `RUSTRON_DEBUG_PAINT=1` (logs the centre pixel after each paint).
- CSP `'self'` does not match `app://app/...` in Servo at this rev (the non-special `app:` URL has an opaque origin). HML's `index.html` has `default-src 'self'; ...` and renders blank under `app://` until the CSP also lists `app:` (e.g. `'self' app:`); with `'self' app:` or no CSP meta, the HML game grid renders. M3 must handle this (scheme-source in the CSP, or a Servo fix).
