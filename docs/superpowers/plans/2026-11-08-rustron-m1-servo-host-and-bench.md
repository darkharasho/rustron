# Rustron M1 — Servo host and parity bench Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a Rust host that shows an app folder in a Servo window over `app://`, plus a bench tool that runs the same app on Rustron and on Electron and prints a pass/fail parity report. Both run on Linux and Windows. The Windows report decides whether to go on building on Servo.

**Architecture:** Cargo workspace with two crates.
- `rustron-host` is a library plus a binary. It uses winit, embeds libservo, and serves `app://app/<path>` from a folder by intercepting Servo's resource loads.
- `rustron-bench` is a binary. It:
  - copies a fixture app folder into a temp run folder and adds a probe script to its `index.html`;
  - launches each engine N times, alternating between them;
  - receives the probe's results over a localhost HTTP endpoint and measures the memory of the whole process tree;
  - writes raw JSONL and a Markdown gate report.

**Tech Stack:** Rust (toolchain pinned to Servo's), libservo at git rev `75d20deef` (the build the 2026-11-08 spike measured, `Servo 0.8.0-75d20deef`), winit, tiny_http, serde/serde_json, sysinfo, url, percent-encoding, and win32job on Windows. GitHub Actions on `ubuntu-latest` and `windows-latest`.

**Spec:** `docs/superpowers/specs/2026-11-08-rustron-design.md`. **Roadmap:** `docs/superpowers/plans/2026-11-08-rustron-stage1-roadmap.md` (this is M1).

## Global Constraints

- Windows and Linux. Nothing may be Linux-only unless it has a Windows counterpart in the same task; macOS is not needed but must not be designed out (`cfg` branches only, no `#[cfg(not(windows))]` that means "Linux").
- No system web views. The renderer is Servo only. CEF is out of M1, but the Servo-specific code stays in `servo_window.rs` behind `run_window`/`run_headless`, so a second engine can sit beside it later.
- Servo is pinned to rev `75d20deef`. The workspace `rust-toolchain.toml` is a copy of Servo's at that rev.
- Heavy commands (Servo builds, test runs) on Michael's machine run as `nice -n 19 taskset -c 0-3 …` with `CARGO_BUILD_JOBS=4`, because the machine freezes under load. A first Servo build takes a long time; don't run two builds at once.
- Parity gate rules (spec "Parity gate"):
  - Times must be "not slower". **Plan decision:** that means `rustron ≤ electron × (1 + tolerance)`, with a default tolerance of `0.05` to absorb run-to-run noise, overridable with `--tolerance`.
  - Memory must be strictly lower.
  - Long frames, errors and failed runs must be no more than Electron's.
- The game fixture (the spike's `~/dev/rustron-spike/www`) holds HML's recorded data, and its covers point at third-party art. **It is never committed.** The bench takes its path as an argument, and `bench/fixtures/` is gitignored.
- License: MIT (plan decision, matching HML), in `LICENSE` and in every crate's `license` field.
- Commit trailer: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **Odd file paths in app folders.** Spaces, non-ASCII letters and percent-encoding, e.g. `app://app/My%20Mods/caf%C3%A9.png`, must load the file. Test in Task 1.
2. **Escaping the app folder.** `..`, `%2e%2e`, backslashes (`..\..\`) and symlinks that point outside the folder all get 403, never file contents. Tests in Task 1.
3. **An engine hangs or crashes during a bench run.** That run is recorded as failed, the bench moves on, and no processes from that run are left alive. Tests in Task 4.
4. **The fixture lacks what the probe looks for** (no search input, no scrollable list). The probe reports empty arrays, and the gate shows that row as `missing` and FAIL instead of panicking or passing. Tests in Task 3.
5. **Windows process trees.** Electron and Servo start child processes (GPU, utility). Memory must sum the whole tree, and killing a run must leave nothing behind. Test in Task 4, run by Windows CI in Task 5.

---

### Task 1: Workspace and the `app://` resolver

**Files:**
- Create: `Cargo.toml` (workspace, members `crates/*`), `rust-toolchain.toml`, `.gitignore` (`target/`, `bench/fixtures/`, `results/`), `LICENSE` (MIT, "Michael Stephens"), `README.md` (one paragraph: what Rustron is, link to spec, "pre-alpha")
- Create: `crates/rustron-host/Cargo.toml`, `crates/rustron-host/src/lib.rs`, `crates/rustron-host/src/app_protocol.rs`
- Test: `crates/rustron-host/tests/app_protocol.rs`, fixture folder `crates/rustron-host/tests/fixtures/app/` containing `index.html`, `assets/app.js`, `My Mods/café.png` (any small PNG), `assets/font.woff2` (any bytes)

**Interfaces:**
- Produces (in `rustron_host::app_protocol`):
  - `pub const SCHEME: &str = "app";` and `pub const HOST: &str = "app";`
  - `pub struct Response { pub status: u16, pub mime: &'static str, pub body: Vec<u8> }`
  - `pub struct AppProtocol { /* root: PathBuf (canonicalized) */ }`
  - `impl AppProtocol { pub fn new(root: &Path) -> std::io::Result<Self>; pub fn respond(&self, url: &url::Url) -> Response }`
  - `pub fn mime_for(path: &Path) -> &'static str`

- [ ] **Step 1: Copy `rust-toolchain.toml` from Servo at rev `75d20deef`** (`https://raw.githubusercontent.com/servo/servo/75d20deef/rust-toolchain.toml`; if the short rev doesn't resolve, get the full hash with `git ls-remote` or the GitHub API). Write the workspace files listed above.

- [ ] **Step 2: Write the failing tests** in `tests/app_protocol.rs`. Build `AppProtocol::new("tests/fixtures/app")` and call `respond(&Url::parse(..))`:
  - `root_serves_index`: `app://app/` → status 200, mime `text/html; charset=utf-8`, and the body equals the bytes of `index.html`
  - `query_and_fragment_ignored`: `app://app/index.html?t0=1&x=y#top` → 200, same body
  - `percent_encoded_unicode_path`: `app://app/My%20Mods/caf%C3%A9.png` → 200, `image/png`
  - `mime_table`: `mime_for` gives `.html` → `text/html; charset=utf-8`, `.js`/`.mjs` → `text/javascript; charset=utf-8`, `.css` → `text/css; charset=utf-8`, `.json` → `application/json`, `.png` → `image/png`, `.jpg`/`.jpeg` → `image/jpeg`, `.webp` → `image/webp`, `.svg` → `image/svg+xml`, `.woff2` → `font/woff2`, `.wasm` → `application/wasm`, and anything else → `application/octet-stream`
  - `missing_file_404`: `app://app/nope.js` → status 404, empty body
  - `dotdot_forbidden`: `app://app/../Cargo.toml`, `app://app/assets/%2e%2e/%2e%2e/Cargo.toml`, `app://app/assets/..%5C..%5CCargo.toml` → all status 403
  - `symlink_escape_forbidden` (`#[cfg(unix)]`, with a `#[cfg(windows)]` twin that uses `std::os::windows::fs::symlink_file` and is skipped when creating the symlink fails without privilege): a symlink created in a temp copy of the fixture pointing at a file outside it → 403
  - `wrong_host_404`: `app://other/index.html` → 404

- [ ] **Step 3: Run them to make sure they fail**
  Run: `cargo test -p rustron-host --test app_protocol`
  Expected: compile errors / FAIL (no `app_protocol` module yet).

- [ ] **Step 4: Implement `app_protocol.rs`.** In `respond`:
  1. Reject any host other than `HOST`.
  2. Percent-decode the path.
  3. Reject it if any segment is `..` after splitting on both `/` and `\`.
  4. Map an empty path or `/` to `index.html`.
  5. Join the result to the root and canonicalize it. If canonicalizing fails, return 404.
  6. Require the canonical path to start with the canonical root; otherwise return 403. This check is what blocks the symlink case.
  7. Read the whole file. M1 doesn't need streaming.

- [ ] **Step 5: Run the tests to make sure they pass**
  Run: `cargo test -p rustron-host --test app_protocol`
  Expected: all PASS.

- [ ] **Step 6: Commit** — `git add -A && git commit -m "feat(host): workspace and app:// resolver"`

---

### Task 2: Servo window and headless runner

**Files:**
- Create: `crates/rustron-host/src/servo_window.rs`, `crates/rustron-host/src/main.rs`
- Modify: `crates/rustron-host/Cargo.toml` (add `servo = { git = "https://github.com/servo/servo", rev = "75d20deef" }`, plus the same `winit` version Servo's `ports/servoshell` uses at that rev, plus `url`), `crates/rustron-host/src/lib.rs`
- Test: `crates/rustron-host/tests/headless.rs`, fixture `crates/rustron-host/tests/fixtures/title-app/index.html` and `crates/rustron-host/tests/fixtures/title-app/main.js`

**Interfaces:**
- Consumes: `AppProtocol::respond`, `SCHEME`, `HOST` (Task 1).
- Produces:
  - `pub struct HostOptions { pub app_dir: PathBuf, pub url: url::Url, pub window_size: (u32, u32) }`
  - `pub fn run_window(opts: HostOptions) -> anyhow::Result<()>`: opens a visible window and returns when it closes.
  - `pub fn run_headless(opts: HostOptions, wait_for_title_prefix: &str, timeout: Duration) -> anyhow::Result<String>`: renders offscreen and returns the first page title that starts with the prefix, or an error on timeout.
  - CLI: `rustron-host --app-dir <dir> [--url <app://app/...>] [--window-size 1200x780]`. `--url` defaults to `app://app/index.html`.

- [ ] **Step 1: Write the failing test.**
  - Fixture `title-app/index.html` loads `<script src="./main.js">`.
  - `main.js` sets `document.title = 'ok:' + location.protocol + ':' + new URLSearchParams(location.search).get('q')`.
  - Test `headless_loads_app_url_and_runs_js`: call `run_headless` with `url = app://app/index.html?q=42`, prefix `ok:` and a 60s timeout, and assert the result is `"ok:app::42"`.
  - Test `headless_times_out_cleanly`: point it at a page that never sets a title, with a 3s timeout, and assert `Err` comes back within 10s.

- [ ] **Step 2: Run it to make sure it fails**
  Run: `nice -n 19 taskset -c 0-3 env CARGO_BUILD_JOBS=4 cargo test -p rustron-host --test headless`
  Expected: FAIL to compile (`run_headless` missing). The first build of Servo is long, so let it finish.

- [ ] **Step 3: Implement `servo_window.rs`**, following `ports/servoshell` at the pinned rev for the exact embedder API names:
  - Rendering: `ServoBuilder` with an event-loop waker; `WindowRenderingContext` for `run_window` and `SoftwareRenderingContext` for `run_headless`.
  - `WebViewBuilder` gets a delegate that does three things:
    - Intercepts loads whose URL scheme is `SCHEME` (`WebViewDelegate::load_web_resource` → `intercept` with `AppProtocol::respond`'s status, `Content-Type` and body).
    - Records title changes, which `run_headless` polls.
    - Requests a repaint when a new frame is ready.
  - Register `app` as a secure scheme the same way servoshell registers its internal schemes, so `fetch` and storage behave as they do on `https`. If that rev has no hook for it, note this in the commit message and in `docs/results/m1-notes.md`; M3 owns `protocol.registerSchemesAsPrivileged`.
  - `main.rs` parses the CLI flags above with `std::env::args` (no clap; three flags).

- [ ] **Step 4: Run the tests to make sure they pass**
  Run: same command as Step 2. Expected: both PASS.

- [ ] **Step 5: Check it by hand.** Run `cargo run -p rustron-host --release -- --app-dir ~/dev/rustron-spike/www`. A 1200×780 window shows HML's game grid (stubbed data) as it looked in the spike. Close it, and check that the process exits with code 0.

- [ ] **Step 6: Commit** — `git commit -am "feat(host): Servo window and headless runner serving app://"` (add new files first).

---

### Task 3: Bench statistics and gate rules

**Files:**
- Create: `crates/rustron-bench/Cargo.toml` (deps `serde`, `serde_json`, `anyhow`), `crates/rustron-bench/src/lib.rs`, `crates/rustron-bench/src/stats.rs`, `crates/rustron-bench/src/gate.rs`
- Test: `crates/rustron-bench/tests/gate.rs`

**Interfaces:**
- Produces (in `rustron_bench`):
  - `ProbeReport`, deserialized with `#[serde(rename_all = "camelCase")]`:
    - `engine: String`
    - `grid_since_launch: f64`, `grid_since_nav: f64`
    - `json_ms: f64`, `sort_ms: f64`, `loop_ms: f64`
    - `frame_ms: Vec<f64>`, `type_ms: Vec<f64>`
    - `errors: u32`
    - `fail: Option<String>`
  - `pub struct RunResult { pub engine: Engine, pub report: Option<ProbeReport>, pub mem_mb: Option<f64>, pub timed_out: bool }`
  - `pub enum Engine { Rustron, Electron }`
  - `stats::median(xs: &[f64]) -> Option<f64>` and `stats::p95(xs: &[f64]) -> Option<f64>`. `p95` uses nearest rank: the sorted value at index `ceil(0.95·n) − 1`. Both return `None` for an empty slice.
  - `pub struct Summary { pub runs: usize, pub completed: usize, pub launch_ms: Option<f64>, pub json_ms: Option<f64>, pub sort_ms: Option<f64>, pub loop_ms: Option<f64>, pub frame_median_ms: Option<f64>, pub long_frames: usize, pub type_median_ms: Option<f64>, pub type_p95_ms: Option<f64>, pub mem_mb: Option<f64>, pub errors: u32 }`
  - `gate::summarize(runs: &[RunResult]) -> Summary`:
    - Each scalar is the median over completed runs. `launch_ms` comes from `grid_since_launch`, and `mem_mb` from `RunResult::mem_mb`.
    - `errors` is the sum over completed runs.
    - `frame_ms` and `type_ms` are pooled across all completed runs before taking the median or p95.
    - `long_frames` counts frames longer than `2.5 ×` that run's own frame median.
    - A run is completed when it has a report, `fail` is `None` and it did not time out.
  - `pub struct GateRow { pub name: &'static str, pub rustron: Option<f64>, pub electron: Option<f64>, pub pass: bool }`
  - `gate::evaluate(r: &Summary, e: &Summary, tolerance: f64) -> Vec<GateRow>`
  - `gate::render_markdown(rows: &[GateRow], header: &str) -> String`

Rows, in order:

| Row | Rule |
|---|---|
| `completed runs` | `r.completed == r.runs`, and also `≥ e.completed` |
| `launch to first screen (ms)` | time rule |
| `scroll frame median (ms)` | time rule |
| `long frames` | `r ≤ e` |
| `typing median (ms)` | time rule |
| `typing p95 (ms)` | time rule |
| `JSON round trip (ms)` | time rule |
| `sort with localeCompare (ms)` | time rule |
| `plain loop (ms)` | time rule |
| `memory (MB)` | `r < e` |
| `JS errors` | `r ≤ e` |

The time rule passes when `r ≤ e × (1 + tolerance)`. Any row where either side is `None` fails, and the report shows it as `missing`.

- [ ] **Step 1: Write the failing tests**
  - `median_odd_even`: `median(&[3.,1.,2.]) == Some(2.0)`, `median(&[1.,2.,3.,4.]) == Some(2.5)`, `median(&[]) == None`
  - `p95_nearest_rank`: for `1..=100` as f64 → `Some(95.0)`; `p95(&[10.0]) == Some(10.0)`; `p95(&[1.,2.,3.,4.,5.,6.,7.,8.,9.,10.,11.,12.,13.,14.]) == Some(14.0)`
  - `spike_numbers_reproduce_known_verdicts`: build Summaries from the spike's medians and run `evaluate` at tolerance `0.05`:
    - Rustron: launch 379, json 57, sort 5.35, loop 64, frame median 8.3, long 0, typing median 11.9, typing p95 19.6, mem 500.
    - Electron: launch 544, json 35.5, sort 2.1, loop 78.5, frame median 8.3, long 0, typing median 14.65, typing p95 16, mem 365.
    - Both: 6 runs, 6 completed, 0 errors.
    - Expected PASS: launch, scroll frame median, long frames, typing median, plain loop, completed runs, JS errors.
    - Expected FAIL: typing p95, JSON, sort, memory.
  - `tolerance_edges`: launch 571.0 vs 544 → pass; 572.0 vs 544 → fail (the boundary 571.2 is avoided because of float rounding); memory 376 vs 376 → fail
  - `failed_rustron_run_fails_gate`: Rustron `runs 6, completed 5` against Electron `6/6` → the `completed runs` row fails
  - `missing_typing_is_fail_not_panic`: completed runs whose `type_ms` is empty → `type_median_ms == None`, both typing rows `pass == false`, and `render_markdown` shows `missing` in those rows
  - `summarize_pools_and_counts`: two completed runs with `frame_ms` `[8,8,8,30]` and `[8,8,8,8]` → `long_frames == 1`; a third run with `timed_out: true` → `runs == 3, completed == 2`

- [ ] **Step 2: Run them to make sure they fail.** Run: `cargo test -p rustron-bench --test gate` → compile FAIL.

- [ ] **Step 3: Implement `stats.rs` and `gate.rs`** with the signatures and rows above. `render_markdown` produces a table with the columns `| Measure | Rustron | Electron | Result |`, numbers to 1 decimal place, `PASS`/`FAIL` in the Result column, and `header` as a paragraph above the table.

- [ ] **Step 4: Run the tests to make sure they pass.** Run: `cargo test -p rustron-bench --test gate` → all PASS.

- [ ] **Step 5: Commit** — `git add -A && git commit -m "feat(bench): stats and parity gate rules"`

---

### Task 4: Probe, launcher, memory and the bench CLI

**Files:**
- Create: `bench/probe.js`, `bench/electron-base/package.json`, `bench/electron-base/main.js`
- Create: `crates/rustron-bench/src/main.rs`, `crates/rustron-bench/src/prepare.rs`, `crates/rustron-bench/src/report_server.rs`, `crates/rustron-bench/src/proc_tree.rs`
- Modify: `crates/rustron-bench/Cargo.toml` (add `tiny_http`, `sysinfo`, `tempfile`, `url`, and `win32job` under `[target.'cfg(windows)'.dependencies]`)
- Test: `crates/rustron-bench/tests/prepare.rs`, `crates/rustron-bench/tests/proc_tree.rs`, `crates/rustron-bench/tests/report_server.rs`

**Interfaces:**
- Consumes: `RunResult`, `ProbeReport`, `Engine`, `summarize`, `evaluate` and `render_markdown` (Task 3); the `rustron-host` binary CLI (Task 2).
- Produces:
  - `prepare::inject_script(html: &str, src: &str) -> String` adds `<script src="{src}"></script>` immediately before the first `<script` tag. If there is none, it adds it before `</head>`; if that is missing too, it prepends it.
  - `prepare::prepare_run_dir(fixture: &Path, probe_js: &str) -> anyhow::Result<tempfile::TempDir>` copies the fixture and writes `__rustron_probe.js`. In the copy's `index.html` it adds the probe *after* the last existing `<script …></script>`, so the fixture's stub loads first.
  - `report_server::ReportServer::start() -> anyhow::Result<ReportServer>` binds `127.0.0.1:0`.
    - `.url() -> String` is `http://127.0.0.1:<port>/report`.
    - `.wait_report(timeout: Duration) -> Option<String>` returns the body of the next POST.
    - Every response, including `OPTIONS`, carries `Access-Control-Allow-Origin: *`.
  - `proc_tree::Spawned`: `spawn(cmd: Command) -> io::Result<Spawned>`, `.pid() -> u32`, `.tree_memory_mb() -> MemReading { mb: f64, kind: &'static str }` and `.kill_tree()`.
    - Linux: PSS summed over `/proc/<pid>/smaps_rollup` for the process and all its descendants; `kind = "PSS"`.
    - Windows: the process runs inside a Job Object with kill-on-close, and memory is the private bytes of every process in the job; `kind = "private bytes"`.
  - CLI:

    ```
    rustron-bench --fixture <dir> --rustron-host <path> --electron <path to electron binary>
                  [--runs 6] [--tolerance 0.05] [--out results/<YYYY-MM-DD>-<os>]
    ```

    It writes `<out>/runs.jsonl`, one `RunResult` per line, and `<out>/report.md`. Before that, the report header lists:
    - the OS and its version;
    - the CPU and RAM;
    - the runs and the tolerance;
    - the memory kind;
    - the Rustron install size: the total of the `rustron-host` binary and any `resources/` folder next to it;
    - the Electron install size: the total of the Electron binary's folder.

- [ ] **Step 1: Write `bench/probe.js`** by porting `~/dev/rustron-spike/www/bench.js`, with these changes:
  - `engine` and `report` (the full report URL) come from the query string, along with `t0`.
  - It posts the raw arrays `frameMs` (240 rAF intervals) and `typeMs` (14 keystrokes) instead of percentiles.
  - It posts with `fetch(report, { method: 'POST', mode: 'no-cors', headers: { 'content-type': 'text/plain' }, body })`.
  - If there is no scrollable element or no input, it sends an empty array.

  Write `bench/electron-base/main.js` from the spike's `scripts/ebase` with these changes:
  - It loads `process.env.RUSTRON_BENCH_URL`.
  - The window is `1200×780`, `frame: false`, `backgroundColor: '#0f191d'`.
  - The screenshot is removed.

- [ ] **Step 2: Write the failing tests**
  - `prepare.rs`:
    - `inject_before_first_script`: `"<head><script src=a.js></script></head>"` with `"p.js"` → the result contains `<script src="p.js"></script><script src=a.js>`.
    - `inject_no_script_uses_head`: the tag lands before `</head>`.
    - `inject_bare_fragment_prepends`.
    - `run_dir_probe_after_last_script`: a fixture `index.html` with two scripts → the probe tag comes after both, and `__rustron_probe.js` exists in the run dir.
  - `report_server.rs`:
    - `receives_post_body`: POST `{"a":1}` with `ureq` (a dev-dependency) → `wait_report(5s) == Some("{\"a\":1}")`, and the response has the CORS header.
    - `times_out_without_post`: `wait_report(300ms) == None`.
  - `proc_tree.rs`:
    - `kills_whole_tree`: spawn a parent that starts a long-lived grandchild and prints the grandchild's pid. Linux: `sh -c 'sleep 600 & echo $!; wait'`. Windows: `powershell -NoProfile -Command "$p = Start-Process ping -ArgumentList '-n','600','127.0.0.1' -PassThru -WindowStyle Hidden; $p.Id; Wait-Process -Id $p.Id"`. Call `kill_tree()`, then assert within 5s that neither pid is alive (check with `sysinfo`).
    - `tree_memory_counts_children`: with the same tree, `tree_memory_mb().mb > 0.0` and `kind` matches the platform.

- [ ] **Step 3: Run them to make sure they fail.** Run: `cargo test -p rustron-bench` → compile FAIL.

- [ ] **Step 4: Implement** `prepare.rs`, `report_server.rs` and `proc_tree.rs` with the interfaces above. Then write `main.rs`. For each run `i` in `0..runs`, engines alternate, Rustron first on even `i`:
  1. Start the `ReportServer`.
  2. Set `t0` to the current epoch ms.
  3. Build the URL.
     - Rustron: `app://app/index.html?engine=rustron&t0=..&report=<url>`, run as `<rustron-host> --app-dir <run dir> --url <that> --window-size 1200x780`.
     - Electron: `file://<run dir>/index.html?engine=electron&t0=..&report=<url>`, run as `<electron> bench/electron-base` with `RUSTRON_BENCH_URL` set.
  4. Spawn the engine and wait up to 45s for a report.
  5. If a report arrives, sleep 3s, then read `tree_memory_mb`.
  6. `kill_tree`, then sleep 2s.
  7. If no report arrived in time, record `timed_out: true`.
  8. Parse failures go into `report: None`, and the raw text is logged to stderr.

- [ ] **Step 5: Run the tests to make sure they pass.** Run: `nice -n 19 taskset -c 0-3 env CARGO_BUILD_JOBS=4 cargo test -p rustron-bench` → all PASS.

- [ ] **Step 6: Run the bench end to end on Linux:**
  `nice -n 19 cargo run -p rustron-bench --release -- --fixture ~/dev/rustron-spike/www --rustron-host target/release/rustron-host --electron ~/Documents/GitHub/hml/node_modules/electron/dist/electron --runs 6`

  Expected:
  - `results/<date>-linux/report.md` exists, with 6 completed runs for each engine.
  - Numbers are in the spike's neighbourhood: launch around 380 vs 540ms, and memory FAIL with covers, as the spike found.
  - No `rustron-host` or `electron` processes remain afterwards (`pgrep -f rustron-host; pgrep -f electron-base` prints nothing).

- [ ] **Step 7: Commit** — `git add -A && git commit -m "feat(bench): probe, launcher, process-tree memory and gate report"` (`results/` is gitignored; Task 6 commits the summaries it needs).

---

### Task 5: Windows and Linux CI

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: everything above.
- Produces: on every push and PR, a matrix job over `ubuntu-latest` and `windows-latest` that:
  1. installs Servo's build prerequisites for that OS at rev `75d20deef`, following that rev's `python/servo/platform/{linux,windows}.py` and Servo's own CI workflow. Linux: its apt package list. Windows: LLVM/clang, MozillaBuild, and anything else listed there;
  2. caches `~/.cargo` and `target/` with `Swatinem/rust-cache`;
  3. runs `cargo test --workspace` (`run_headless` needs no display);
  4. builds `cargo build --release -p rustron-host -p rustron-bench`;
  5. uploads an artifact `rustron-m1-<os>` with the two binaries and whatever runtime files the host needs next to it (Servo `resources/`, if the pinned rev still loads them from disk).

- [ ] **Step 1: Write `ci.yml`** as described. Set `timeout-minutes: 180`, because a cold Servo build is long.
- [ ] **Step 2: Push and watch the run.** This needs a GitHub remote. Creating the public `darkharasho/rustron` repo is outward-facing, so ask Michael before running `gh repo create darkharasho/rustron --public --source . --push`. After pushing, show the run with the CI watcher card.
  Expected: both jobs green. The Windows job must run and pass `kills_whole_tree` and `tree_memory_counts_children`.
- [ ] **Step 3: Fix whatever the Windows build reveals.** The usual culprits are paths, `cfg` gaps and missing `win32job` handling. Repeat until both jobs are green, and commit each fix with a `fix(ci): …` message.

---

### Task 6: The Windows go/no-go run

**Files:**
- Create: `docs/results/2026-11-m1-linux.md`, `docs/results/2026-11-m1-windows.md`, `docs/results/m1-decision.md`

**Interfaces:**
- Consumes: the CI artifact `rustron-m1-windows` (Task 5) and the bench CLI (Task 4).

- [ ] **Step 1: Get the fixture to the Windows machine.** Pack it with `tar czf ~/rustron-fixture.tgz -C ~/dev/rustron-spike www`, and hand the file to Michael directly. It is never uploaded anywhere public.
- [ ] **Step 2: Have Michael run it on the Windows machine.** Steps to give him:
  1. Unzip the artifact.
  2. Extract the fixture.
  3. Run `npm i electron@44.7.2` in an empty folder, to match HML's Electron version.
  4. Run `rustron-bench.exe --fixture <www> --rustron-host <path>\rustron-host.exe --electron <folder>\node_modules\electron\dist\electron.exe --runs 6`.
  5. Send back `report.md` and `runs.jsonl`.

  Before the run, he closes games and other heavy apps and notes the display scaling.
- [ ] **Step 3: Write `docs/results/2026-11-m1-{linux,windows}.md`.** Each has the bench's `report.md`, plus a screenshot of HML in each engine, and notes on anything that rendered wrong.
- [ ] **Step 4: Write `docs/results/m1-decision.md`, applying this rule (plan decision).** **Go** on Servo if, on Windows:
  - HML renders with no breakage a user would notice;
  - `completed runs`, `launch`, `scroll frame median`, `long frames` and `JS errors` pass.

  The known must-fix rows (memory with covers, typing p95, JSON, sort) may fail; they belong to M4. They are only recorded, with each failure and its size listed as M4 input.
  **No-go** if any of the go rows fail on Windows. In that case the file lists the failures, and the next step is a conversation with Michael about whether to do Windows-specific Servo work or move CEF earlier. Not a quiet change of plan.
- [ ] **Step 5: Commit** — `git add docs/results && git commit -m "docs: M1 results and go/no-go"`. Then tell Michael the verdict along with the two report tables.
