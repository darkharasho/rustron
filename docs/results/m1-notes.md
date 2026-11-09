# M1 notes

## Servo embedding (Task 2)

- Servo is pinned at git rev `75d20deef` (`Servo 0.8.0`). The workspace `Cargo.lock` was seeded from Servo's own `Cargo.lock` at that rev so the transitive dependency versions match what Servo builds with.
- `servo` is used with `default-features = false, features = ["bundled", "js_jit"]` (no multiprocess, webgl, webgpu, webxr, devtools, media). Windows adds `no-wgl` like servoshell does.
- `app://app/...` is served through `WebViewDelegate::load_web_resource` -> `WebResourceLoad::intercept`, backed by `AppProtocol::respond`. The request interceptor in `net` runs before scheme dispatch, so it sees every `app:` fetch, including subresources and `fetch()`.
- Secure scheme: Servo has a hook, `ProtocolHandler::is_secure()` (registered through `ServoBuilder::protocol_registry`). `app` is registered with `is_secure() = true` and `is_fetchable() = true`. At this rev `is_secure` only bypasses mixed-content checks, so `window.isSecureContext` and secure-context-only APIs (e.g. `crypto.subtle`, service workers) are not yet guaranteed to behave as on `https`. Not verified in M1; M3 owns `protocol.registerSchemesAsPrivileged` and should check this.
- Servo can be initialised once per process (its `Opts` are a process-wide global that panics on a second init). The headless tests therefore re-run each case in a child process of the same test binary.
- The window runner forwards mouse move/click/wheel and resize only. Keyboard input is not forwarded yet.
