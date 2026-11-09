use std::path::PathBuf;
use std::process::ExitCode;

use rustron_host::app_protocol::{HOST, SCHEME};
use rustron_host::servo_window::{run_window, HostOptions};

const USAGE: &str =
    "usage: rustron-host --app-dir <dir> [--url <app://app/...>] [--window-size 1200x780]";

fn parse_size(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once('x')?;
    let (w, h) = (w.parse().ok()?, h.parse().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<HostOptions, String> {
    let mut app_dir: Option<PathBuf> = None;
    let mut url = format!("{SCHEME}://{HOST}/index.html");
    let mut window_size = (1200, 780);
    let mut args = args;
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--app-dir" => app_dir = Some(PathBuf::from(value()?)),
            "--url" => url = value()?,
            "--window-size" => {
                let v = value()?;
                window_size = parse_size(&v).ok_or_else(|| format!("bad --window-size {v:?}, want WxH"))?;
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(HostOptions {
        app_dir: app_dir.ok_or("--app-dir is required")?,
        url: url::Url::parse(&url).map_err(|e| format!("bad --url {url:?}: {e}"))?,
        window_size,
    })
}

fn main() -> ExitCode {
    let opts = match parse_args(std::env::args().skip(1)) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("rustron-host: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run_window(opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("rustron-host: {e:#}");
            ExitCode::FAILURE
        }
    }
}
