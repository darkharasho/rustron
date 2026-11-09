//! Headless Servo tests (software rendering, no display needed).

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use rustron_host::servo_window::{run_headless, HostOptions};

const CHILD_ENV: &str = "RUSTRON_HEADLESS_TEST_CHILD";

/// Servo can only be initialised once per process (its options are a process-wide global),
/// so each case re-runs itself in a fresh child process of this same test binary.
fn isolated(name: &str, body: impl FnOnce()) {
    if std::env::var_os(CHILD_ENV).is_some() {
        body();
        return;
    }
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--test-threads=1", "--nocapture"])
        .env(CHILD_ENV, "1")
        .output()
        .expect("spawn child test process");
    assert!(
        out.status.success(),
        "child failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn opts(app: &str, url: &str) -> HostOptions {
    HostOptions {
        app_dir: fixture(app),
        url: url::Url::parse(url).unwrap(),
        window_size: (800, 600),
    }
}

#[test]
fn headless_loads_app_url_and_runs_js() {
    isolated("headless_loads_app_url_and_runs_js", || {
        let title = run_headless(
            opts("title-app", "app://app/index.html?q=42"),
            "ok:",
            Duration::from_secs(60),
        )
        .expect("title");
        assert_eq!(title, "ok:app::42");
    });
}

#[test]
fn headless_times_out_cleanly() {
    isolated("headless_times_out_cleanly", || {
        let start = Instant::now();
        let res = run_headless(
            opts("blank-app", "app://app/index.html"),
            "ok:",
            Duration::from_secs(3),
        );
        assert!(res.is_err(), "expected timeout error, got {res:?}");
        assert!(start.elapsed() < Duration::from_secs(10));
    });
}
