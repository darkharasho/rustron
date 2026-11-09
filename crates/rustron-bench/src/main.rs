use anyhow::{anyhow, Context, Result};
use rustron_bench::prepare::prepare_run_dir;
use rustron_bench::proc_tree::{spawn, MEM_KIND};
use rustron_bench::report_server::ReportServer;
use rustron_bench::{evaluate, render_markdown, summarize, Engine, ProbeReport, RunResult};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PROBE_JS: &str = include_str!("../../../bench/probe.js");
// The Electron launcher app is embedded and written out per run, so the shipped binary does not
// depend on the source tree it was built from.
const ELECTRON_BASE_PACKAGE_JSON: &str = include_str!("../../../bench/electron-base/package.json");
const ELECTRON_BASE_MAIN_JS: &str = include_str!("../../../bench/electron-base/main.js");

fn write_electron_base(run_dir: &Path) -> Result<PathBuf> {
    let base = run_dir.join(".rustron-electron-base");
    std::fs::create_dir_all(&base)?;
    std::fs::write(base.join("package.json"), ELECTRON_BASE_PACKAGE_JSON)?;
    std::fs::write(base.join("main.js"), ELECTRON_BASE_MAIN_JS)?;
    Ok(base)
}

struct Args {
    fixture: PathBuf,
    rustron_host: PathBuf,
    electron: PathBuf,
    runs: usize,
    tolerance: f64,
    out: PathBuf,
}

fn parse_args() -> Result<Args> {
    let mut it = std::env::args().skip(1);
    let (mut fixture, mut host, mut electron, mut out) = (None, None, None, None);
    let (mut runs, mut tolerance) = (6usize, 0.05f64);
    while let Some(a) = it.next() {
        let mut val = || it.next().ok_or_else(|| anyhow!("{a} needs a value"));
        match a.as_str() {
            "--fixture" => fixture = Some(PathBuf::from(val()?)),
            "--rustron-host" => host = Some(PathBuf::from(val()?)),
            "--electron" => electron = Some(PathBuf::from(val()?)),
            "--runs" => runs = val()?.parse()?,
            "--tolerance" => tolerance = val()?.parse()?,
            "--out" => out = Some(PathBuf::from(val()?)),
            other => return Err(anyhow!("unknown argument {other}")),
        }
    }
    Ok(Args {
        fixture: fixture.ok_or_else(|| anyhow!("--fixture is required"))?,
        rustron_host: host.ok_or_else(|| anyhow!("--rustron-host is required"))?,
        electron: electron.ok_or_else(|| anyhow!("--electron is required"))?,
        runs,
        tolerance,
        out: out.unwrap_or_else(|| PathBuf::from(format!("results/{}-{}", today(), std::env::consts::OS))),
    })
}

fn today() -> String {
    let days = (SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() / 86400) as i64;
    // civil-from-days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn dir_size(p: &Path) -> u64 {
    let Ok(md) = std::fs::symlink_metadata(p) else { return 0 };
    if md.is_dir() {
        std::fs::read_dir(p).map_or(0, |rd| rd.flatten().map(|e| dir_size(&e.path())).sum())
    } else {
        md.len()
    }
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()
}

fn run_once(args: &Args, engine: Engine) -> Result<RunResult> {
    let run_dir = prepare_run_dir(&args.fixture, PROBE_JS)?;
    let server = ReportServer::start()?;
    let t0 = now_ms();
    let (name, cmd) = match engine {
        Engine::Rustron => {
            let mut u = url::Url::parse("app://app/index.html")?;
            u.query_pairs_mut()
                .append_pair("engine", "rustron")
                .append_pair("t0", &t0.to_string())
                .append_pair("report", &server.url());
            let mut c = Command::new(&args.rustron_host);
            c.arg("--app-dir").arg(run_dir.path()).arg("--url").arg(u.as_str()).args(["--window-size", "1200x780"]);
            ("rustron", c)
        }
        Engine::Electron => {
            let mut u = url::Url::from_file_path(run_dir.path().join("index.html"))
                .map_err(|_| anyhow!("run dir path is not absolute"))?;
            u.query_pairs_mut()
                .append_pair("engine", "electron")
                .append_pair("t0", &t0.to_string())
                .append_pair("report", &server.url());
            let base = write_electron_base(run_dir.path())?;
            let mut c = Command::new(&args.electron);
            // Electron 44 hangs before 'ready' on native Wayland on the dev machine; XWayland works.
            // The ozone platform has to be chosen on the command line, before main.js runs.
            #[cfg(target_os = "linux")]
            c.arg("--ozone-platform=x11");
            c.arg(&base).env("RUSTRON_BENCH_URL", u.as_str());
            ("electron", c)
        }
    };
    let mut cmd = cmd;
    cmd.stdin(Stdio::null()).stdout(Stdio::null());
    let mut proc = spawn(cmd).with_context(|| format!("spawning {name}"))?;
    let body = server.wait_report(Duration::from_secs(45));
    let mut result = RunResult { engine, report: None, mem_mb: None, timed_out: body.is_none() };
    if let Some(body) = &body {
        std::thread::sleep(Duration::from_secs(3));
        result.mem_mb = Some(proc.tree_memory_mb().mb);
        match serde_json::from_str::<ProbeReport>(body) {
            Ok(r) => result.report = Some(r),
            Err(e) => eprintln!("{name}: unparseable report ({e}): {body}"),
        }
    } else {
        eprintln!("{name}: no report within 45s");
    }
    proc.kill_tree();
    std::thread::sleep(Duration::from_secs(2));
    Ok(result)
}

fn header(args: &Args) -> String {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_all();
    let cpu = sys.cpus().first().map_or("unknown".to_string(), |c| c.brand().trim().to_string());
    let host_dir = args.rustron_host.parent().unwrap_or(Path::new("."));
    let rustron_size = std::fs::metadata(&args.rustron_host).map_or(0, |m| m.len()) + dir_size(&host_dir.join("resources"));
    let electron_size = args.electron.parent().map_or(0, dir_size);
    format!(
        "# Rustron vs Electron parity gate\n\n- OS: {} {}\n- CPU: {} ({} threads)\n- RAM: {:.1} GB\n- Runs per engine: {}\n- Tolerance: {}\n- Memory kind: {}\n- Rustron install size: {:.1} MB\n- Electron install size: {:.1} MB",
        sysinfo::System::name().unwrap_or_default(),
        sysinfo::System::os_version().unwrap_or_default(),
        cpu,
        sys.cpus().len(),
        sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0),
        args.runs,
        args.tolerance,
        MEM_KIND,
        mb(rustron_size),
        mb(electron_size),
    )
}

fn main() -> Result<()> {
    let args = parse_args().unwrap_or_else(|e| {
        eprintln!("usage: rustron-bench --fixture <dir> --rustron-host <path> --electron <path> [--runs 6] [--tolerance 0.05] [--out <dir>]\n{e}");
        std::process::exit(2);
    });
    std::fs::create_dir_all(&args.out)?;
    let mut all: Vec<RunResult> = Vec::new();
    for i in 0..args.runs {
        let order = if i % 2 == 0 { [Engine::Rustron, Engine::Electron] } else { [Engine::Electron, Engine::Rustron] };
        for engine in order {
            eprintln!("run {}/{} {:?}", i + 1, args.runs, engine);
            let r = run_once(&args, engine).unwrap_or_else(|e| {
                eprintln!("run failed: {e:#}");
                RunResult { engine, report: None, mem_mb: None, timed_out: true }
            });
            all.push(r);
        }
    }
    let lines: Vec<String> = all.iter().map(serde_json::to_string).collect::<Result<_, _>>()?;
    std::fs::write(args.out.join("runs.jsonl"), lines.join("\n") + "\n")?;
    let pick = |e: Engine| all.iter().filter(|r| r.engine == e).cloned().collect::<Vec<_>>();
    let rows = evaluate(&summarize(&pick(Engine::Rustron)), &summarize(&pick(Engine::Electron)), args.tolerance);
    let md = render_markdown(&rows, &header(&args));
    std::fs::write(args.out.join("report.md"), &md)?;
    println!("{md}");
    Ok(())
}
