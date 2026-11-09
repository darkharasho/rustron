# Rustron — design

Date: 2026-11-08. Status: draft, waiting for approval.

## What it is, and why

Rustron is a drop-in replacement for Electron, with the host written in Rust. An Electron app moves to it by changing its build configuration, not its code. `import { app, BrowserWindow, ipcMain } from 'electron'` keeps working, the preload script keeps working, and the renderer keeps working.

Why: memory use and download size. A small Electron app ships about 300MB unpacked; HML's own code is 18MB of that. A blank Electron window uses about 277MB of memory. Michael (the owner) maintains 22 Electron apps, mostly used on Windows with a sizeable Linux and Steam Deck share, and every one of them pays that cost.

Rustron is public open source, but **Michael's own apps come first**. Features, ordering and bug priority are decided by what those 22 apps need. Other people's apps are welcome but don't set priorities.

## Goals

1. All 22 of Michael's Electron apps (listed below) run on Rustron with **no app code changes**, only build configuration changes.
2. Smaller and lighter than Electron: smaller installers, smaller unpacked size, less memory per app.
3. Windows and Linux first. macOS comes later and must not be designed out.
4. A `rustron` command line tool that replaces electron-builder and produces installers that work with the apps' existing auto-update flow.

## Non-goals

- macOS in the first release.
- Supporting every Electron API. Rustron supports what the 22 apps use (listed below) and grows from there.
- Promising more on Wayland than Electron does. Where Electron is limited on Wayland (global shortcuts, window positioning, click-through, always-on-top), Rustron matches it and goes no further.
- Using the operating system's web view (WebView2, WebKitGTK). Every platform bundles its own engine so apps render the same everywhere.
- Maintaining a Chromium fork.
- Replacing electron-vite. It stays the development tool.

## Anti-goal: it must not be slower or buggier than Electron

In Michael's words, the project fails "if it's slower or buggier than electron", even if it is smaller. That turns into two hard rules:

1. **Per-app parity gate.** An app ships on Rustron only when, on the same machine, it is at least as fast and as stable as the same app on Electron. The measurements are listed under "Parity gate".
2. **One-line escape hatch.** Any app that fails the gate on the Servo engine can switch to the CEF (Chromium) engine with one line of config, and keep everything else about Rustron. An app that hasn't passed on either engine stays on Electron.

## Architecture (decided)

### Two engines

- **Servo is the main engine.** It renders the windows (the renderer process side).
- **CEF is the fallback engine,** behind the same engine interface, picked per app in config. It exists for apps that need camera/microphone, WebRTC, embedded third-party websites or other things Servo can't do well yet, and as the escape hatch for any app that fails the parity gate on Servo.
- Both engines are bundled with the app. No system web views on any platform.
- The engine interface is designed so the Electron API layer doesn't know which engine is underneath. Engine-specific features (for example `<webview>` and `WebContentsView`, which are CEF-only at first) fail with a clear error on Servo.

### Processes (Claude's call, Michael asked for a recommendation)

Rustron uses Electron's process layout:
- **The main process** runs the app's main-process JavaScript and the Node layer.
- **Each window gets its own renderer process**, so a crash in one window doesn't take down the app or the other windows. Renderer crashes raise the same events Electron raises (`render-process-gone`), so apps' existing crash handling keeps working.

Why: "buggier than Electron" counts as failure, and a single shared process would let one bad window crash the whole app. The cost is memory. The spike's numbers were close to a single process, so each extra window will cost more than those numbers suggest. The parity gate still requires lower memory than Electron, and the separate processes are counted in that measurement.

Servo's multi-process mode and its sandbox are less mature than Chromium's, especially on Windows. Making them solid is part of the project, not optional.

### Users' data carries over (decided)

When an app moves from Electron to Rustron, users keep their data:
- localStorage, IndexedDB and cookies (sessions and logins);
- secrets stored with `safeStorage`;
- everything under the app's `userData` folder, which keeps the same path.

On first launch after the update, Rustron imports Chromium's on-disk formats. It decrypts `safeStorage` data the way Chromium does: DPAPI on Windows, and the libsecret or kwallet key on Linux. HML cannot ship on Rustron until this works for the data HML has.

Claude's call: the import copies the data and never changes or deletes Electron's original files. If an app goes back to Electron, users return to the state they were in before the switch. Changes made while on Rustron do not carry back.

### One JavaScript engine

Main-process JavaScript runs on **SpiderMonkey, the same engine Servo already embeds**. Rustron does not ship V8, Node or Deno. The spike showed that adding Deno/V8 next to Servo costs about 74MB of size and 60MB of memory, which defeats the purpose.

A **Node-compatible layer written in Rust** sits on SpiderMonkey and provides the Node built-ins the apps use:

- `fs`, `path`, `os`, `url`, `events`, `stream`, `buffer`, `util`, `crypto`, `zlib`, `timers`
- `child_process`, with pipes, signals and detached processes
- `net`, `http`, `https`: clients and servers, plus unix sockets (dbus-next needs them)
- `worker_threads`, `fetch`, `WebSocket`
- CommonJS `require` and ES modules, with Node's resolution rules, so `node_modules` works unchanged

It must also run **Node-API (N-API) native addons**. About half of the apps need them: node-pty, node-termios, better-sqlite3, sqlite-vec, onnxruntime-node, @lancedb/lancedb, sharp, argon2, uiohook-napi, @discordjs/opus, axilog (built with napi-rs), nut-js and mica-electron. Bun's Node-API on JavaScriptCore shows this can be done on a non-V8 engine. Addons are loaded from the prebuilt `.node` files the packages already ship (Node-API is ABI-stable, so no rebuild per Rustron version).

npm packages the apps depend on that must work unmodified: electron-updater, electron-store, electron-log.

### Electron API surface (the union of what the 22 apps use)

- **Windows:** `BrowserWindow`, multiple windows, frameless, transparent, always-on-top, click-through (`setIgnoreMouseEvents`), bounds, minimize/maximize/fullscreen, the usual events.
- **Renderer bridge:** preload scripts, `contextBridge`, `ipcMain`/`ipcRenderer` (`send`, `invoke`/`handle`, `on`), `webContents.send`, `executeJavaScript`, `capturePage`.
- **Desktop integration:** `Tray`, `Menu`, `globalShortcut`, `Notification`, `dialog`, `clipboard`, `shell`, `safeStorage`, `nativeImage`, deep links / protocol client, single-instance lock, login items, `powerMonitor`, `screen`.
- **App plumbing:** `app` lifecycle and paths, `protocol.handle`, `session` with partitions, `webRequest`, `desktopCapturer`, `utilityProcess`.
- **CEF only at first:** `WebContentsView`, `BaseWindow`, `<webview>`.

### Fixes Servo must get (must-fix, from the spike)

These are known gaps that would fail the parity gate. They are part of the project, not optional:

1. **Decoded image memory.** Servo keeps every decoded image in memory. With HML's 338 game covers it used about 500MB against Electron's 365MB, reversing Servo's advantage. Servo must drop decoded images that are off screen and decode them again when needed, as Chromium does.
2. **JSON speed.** SpiderMonkey was 1.6× slower than V8 at a 1.1MB `JSON.parse`/`JSON.stringify` round trip.
3. **`localeCompare` / ICU collation speed.** Sorting 338 names with `localeCompare` was 2.5× slower.
4. **Typing hitches.** Keystroke-to-frame was faster than Electron on average (11.9 vs 14.7ms) but the 95th percentile was worse (19.6 vs 16ms, worst 42ms).

Fixes go upstream to Servo/SpiderMonkey where they will be accepted. Where they won't be, Rustron carries patches on a pinned Servo version.

## Tooling (decided)

- A **`rustron` CLI** replaces electron-builder. It reads the app's existing electron-builder config (`electron-builder.yml`/`.json` or the `build` key in `package.json`), so apps don't rewrite their packaging config.
- It produces **AppImage** on Linux and **NSIS** installers on Windows, plus the `latest.yml` / `latest-linux.yml` files electron-updater reads, so the existing GitHub Releases update flow keeps working. Updating from an Electron build to a Rustron build of the same app must work through the normal updater.
- **Windows signing** uses Azure Artifact Signing (account `darkharasho-signing`, profile `public-trust`, publisher `Michael Stephens`), as the apps do today. See `~/.claude/templates/windows-signing/README.md`; HML is the reference.
- **electron-vite stays** for development. `rustron dev` (or equivalent) runs the electron-vite dev server output on Rustron, with hot reload in the renderer.
- Engine choice is one config line per app, e.g. `rustron.engine: servo | cef`.

## Parity gate

Before an app switches to Rustron for its users, it is measured on the same machine against its own Electron build, on Windows and Linux:

| Measure | Pass |
|---|---|
| Launch to first useful screen | not slower |
| Scrolling the app's main list | same frame rate, no more long frames |
| Keystroke to screen update | median and 95th percentile not worse |
| The app's heavy main-process work (its own benchmark per app) | not slower |
| Memory after a typical session | lower |
| Installer and unpacked size | smaller |
| The app's own test suite and a manual run of its main features | no new failures; no crashes or errors that Electron doesn't have |

**Apps on CEF (decided):** the size and memory rows don't apply. An app on CEF passes the gate when it is as fast and as stable as on Electron. That keeps CEF a working escape hatch, even though those apps won't be smaller.

The spike harness in `~/dev/rustron-spike` (throwaway) shows the shape of these measurements.

## Rollout order (decided)

Apps move one at a time. An app that hasn't moved stays on Electron. Repos are in `~/Documents/GitHub/` unless noted.

1. **Fit as they are (7) — Servo, no native addons.** HML first (`hml`), then `axiadmin`, `axiam`, `axiom`, `axiroster`, `wallpaper-shuffler`, `text-searcher`.
2. **Need Node-API (9) — Servo plus native addons.** `axibridge`, `axipulse`, `sai`, `tai`, `LinuxGSMUI`, `otto`, `axivale`, `AutoMancer`, `BotCord`.
3. **Need media or embedded browsing (6) — CEF first.** `insta360-link-linux-controller`, `duel-binder`, `duel-binder-pokedex`, `glkvm-linux-wrapper`, `TopStatsAIO`, `axiforge`. These can move to Servo later if it catches up.

## What we know from the spike (2026-11-08, Linux, Servo nightly, HML's game grid)

- Servo rendered HML almost identically to Electron.
- Memory (PSS): blank page 153MB vs Electron 277MB; HML without images 227 vs 376MB; HML with 338 covers about 500 vs 365MB (the image problem above).
- Size: stripped servoshell 103MB vs HML's Electron build at 302MB unpacked.
- Speed, median of 6 launches: launch to grid 379 vs 544ms; scrolling 120fps on both with no long frames; JSON and `localeCompare` slower, plain loops 20% faster on SpiderMonkey.
- Caveats: Linux only, one screen, simulated input, a game running in the background, and main-process speed was not measured directly.

## Calls made by Claude (not yet confirmed by Michael)

- **The first plan covers stage 1 only.** That means HML shipping on Rustron on Windows and Linux, with the Node layer, the Servo fixes, the CLI and the parity gate built only as far as HML needs them. Later stages get their own plans and grow these parts.
- **Who builds it:** Michael, working with AI agents, not a team. Milestones are cut small enough that each one ends with something you can run.
- **Windows proof comes early.** The spike ran only on Linux, so the first milestone runs HML's renderer in Servo on Windows and measures it before any deep work starts.
- **Gate machines:** a Windows desktop and a Linux desktop, on both X11 and Wayland, plus a Steam Deck in Gaming Mode for apps people use there (HML first).

## Left to the builder

- Crate layout, internal APIs, and how the engine interface is shaped.
- How Servo is embedded (libservo directly vs. a fork) and how the Servo version is pinned and updated.
- How the Node layer is implemented, and how much is borrowed from existing Rust projects.
- The IPC transport between the main process and renderers.
- Which CEF build is used and how it is fetched.
- The order of work inside each rollout stage, and how the parity gate is automated.
- Project name checks, license, and repository hosting details beyond "public open source".
