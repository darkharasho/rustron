# Rustron stage 1 (HML) — roadmap

**Spec:** `docs/superpowers/specs/2026-11-08-rustron-design.md`

Stage 1 ends when HML ships to its users on Rustron (Servo engine) on Windows and Linux, having passed the parity gate. The stage is too large for one plan, so it is split into milestones. Each milestone produces software that runs and is tested, and **each gets its own detailed plan when it starts**, written against what the previous milestone actually produced. Only M1 has a detailed plan so far: `2026-11-08-rustron-m1-servo-host-and-bench.md`.

What HML needs (surveyed 2026-11-08 from `~/Documents/GitHub/hml/src/main` and `src/preload`):
- **Node built-ins:** `fs` (also `fs/promises`), `path`, `crypto`, `child_process`, `util`, `os`, `zlib`, `url`, `net` (server and client).
- **npm runtime dependencies:** electron-updater, @electron-toolkit/utils, fflate, js-yaml, marked, zod, dompurify and 7zip-bin. All are pure JS; 7zip-bin ships a native executable that HML runs, not a `.node` addon.
- **Electron API:**
  - `app`: lifecycle, `getPath`, `isPackaged`, single-instance lock, `setAsDefaultProtocolClient`, `getVersion`, `quit`/`exit`.
  - `BrowserWindow`: frameless with window controls, focus/maximize events, `fromWebContents`, `getAllWindows`.
  - `webContents`: `send`, `on`, `setWindowOpenHandler`, and the edit commands (cut, copy, paste, selectAll).
  - IPC: `ipcMain.handle`, `ipcRenderer.invoke`/`on`/`removeListener`, `contextBridge.exposeInMainWorld`.
  - `dialog`: open and save dialogs.
  - `shell`: `openPath`, `openExternal`.
  - `safeStorage`: encrypt/decrypt, `isEncryptionAvailable`, `getSelectedStorageBackend`.
  - `protocol`: `registerSchemesAsPrivileged`, `handle`.
  - `Notification.isSupported`, `nativeImage.createFromBuffer`.
- **Renderer storage:** localStorage (theme accent). The GitHub token is a `safeStorage` blob that HML stores itself.

## Milestones

**M1 — Servo host and the parity bench.**
- A Rust host opens a Servo window and serves an app folder over `app://`.
- A bench tool measures it against Electron.
- Both run on Linux and Windows, with Windows CI.
- Michael runs the bench on a Windows machine. Its report is the go/no-go for building the rest on Servo.

**M2 — Main-process JavaScript.**
- SpiderMonkey (the `mozjs` build Servo already links) runs in the host's main process, with a Tokio event loop.
- Module loading: CommonJS and ESM, with Node's resolution rules.
- Timers, `process` and `Buffer`.
- The built-ins listed above, in HML's order of use: `fs`, `path`, `crypto`, `child_process`, `util`, `os`, `zlib`, `url`, `net`.
- Exit test: HML's main-process modules with no Electron imports run under Rustron, and their vitest unit tests pass when run against Rustron's runtime.
- Conformance: a curated subset of Node's own `test/parallel` tests for each built-in.

**M3 — Electron API, processes and IPC.**
- The main process plus one renderer process per window, with `render-process-gone` on renderer crashes.
- Preload scripts, `contextBridge` and the IPC calls listed above.
- `BrowserWindow`, frameless window controls, `app`, `dialog`, `shell`, `protocol.handle`, the single-instance lock and deep links.
- Exit test: HML's real `out/` build runs on Rustron with no fixture stub, and its main features work by hand.

**M4 — Servo must-fixes.**
- Drop off-screen decoded images, so memory with HML's 338 covers ends up below Electron.
- JSON parse/stringify speed.
- `localeCompare` (ICU collation) speed.
- Typing p95 hitches.
- Each fix is measured with the M1 bench, sent upstream where possible, and otherwise kept as a patch on the pinned Servo.

**M5 — Users' data carries over.**
- On first launch, copy HML's Electron localStorage (Chromium LevelDB) into Servo's storage. This also covers cookies and IndexedDB, though HML has none worth keeping; the importer is generic and tested on HML.
- `safeStorage` must decrypt blobs Electron encrypted. This is a format match, not an import:
  - Windows: the `v10` prefix, AES-256-GCM, with the key DPAPI-wrapped in `Local State`.
  - Linux: the `v11` prefix, AES-128-CBC, with the key from the libsecret or kwallet "Chromium Safe Storage" secret, PBKDF2 at 1 iteration, salt `saltysalt`.
- Electron's original files are never changed or deleted.

**M6 — `rustron` CLI and updates.**
- Reads `electron-builder.yml`.
- Builds an AppImage and an NSIS installer, plus `latest.yml` and `latest-linux.yml`.
- Signs on Windows with Azure Artifact Signing (`darkharasho-signing` / `public-trust`).
- electron-updater runs on Rustron.
- Exit test: a published Electron HML updates itself to a Rustron HML through the normal updater, and the user's token and theme survive.

**M7 — Parity gate and ship.**
- The full gate on a Windows desktop, Linux X11, Linux Wayland and a Steam Deck in Gaming Mode.
- Any failures go back to the milestone that owns them.
- HML's release switches to Rustron, with the CEF escape hatch not needed.

CEF (the fallback engine) is not in stage 1. Stage 1 only keeps the engine boundary in the host clean (M1, M3) so CEF can be added in stage 3.
