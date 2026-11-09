use crate::stats::{median, p95};
use crate::RunResult;

#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub runs: usize,
    pub completed: usize,
    pub launch_ms: Option<f64>,
    pub json_ms: Option<f64>,
    pub sort_ms: Option<f64>,
    pub loop_ms: Option<f64>,
    pub frame_median_ms: Option<f64>,
    pub long_frames: usize,
    pub type_median_ms: Option<f64>,
    pub type_p95_ms: Option<f64>,
    pub mem_mb: Option<f64>,
    pub errors: u32,
}

pub fn summarize(runs: &[RunResult]) -> Summary {
    let done: Vec<&RunResult> = runs
        .iter()
        .filter(|r| !r.timed_out && r.report.as_ref().is_some_and(|p| p.fail.is_none()))
        .collect();
    let reports: Vec<_> = done.iter().filter_map(|r| r.report.as_ref()).collect();
    let med = |f: &dyn Fn(&crate::ProbeReport) -> f64| {
        median(&reports.iter().map(|p| f(p)).collect::<Vec<_>>())
    };
    let frames: Vec<f64> = reports.iter().flat_map(|p| p.frame_ms.iter().copied()).collect();
    let types: Vec<f64> = reports.iter().flat_map(|p| p.type_ms.iter().copied()).collect();
    let long_frames = reports
        .iter()
        .map(|p| match median(&p.frame_ms) {
            Some(m) => p.frame_ms.iter().filter(|&&f| f > 2.5 * m).count(),
            None => 0,
        })
        .sum();
    let mems: Vec<f64> = done.iter().filter_map(|r| r.mem_mb).collect();
    Summary {
        runs: runs.len(),
        completed: done.len(),
        launch_ms: med(&|p| p.grid_since_launch),
        json_ms: med(&|p| p.json_ms),
        sort_ms: med(&|p| p.sort_ms),
        loop_ms: med(&|p| p.loop_ms),
        frame_median_ms: median(&frames),
        long_frames,
        type_median_ms: median(&types),
        type_p95_ms: p95(&types),
        mem_mb: median(&mems),
        errors: reports.iter().map(|p| p.errors).sum(),
    }
}

#[derive(Debug, Clone)]
pub struct GateRow {
    pub name: &'static str,
    pub rustron: Option<f64>,
    pub electron: Option<f64>,
    pub pass: bool,
}

pub fn evaluate(r: &Summary, e: &Summary, tolerance: f64) -> Vec<GateRow> {
    let time = |name, rv: Option<f64>, ev: Option<f64>| GateRow {
        name,
        rustron: rv,
        electron: ev,
        pass: matches!((rv, ev), (Some(a), Some(b)) if a <= b * (1.0 + tolerance)),
    };
    let le = |name, rv: f64, ev: f64| GateRow {
        name,
        rustron: Some(rv),
        electron: Some(ev),
        pass: rv <= ev,
    };
    vec![
        GateRow {
            name: "completed runs",
            rustron: Some(r.completed as f64),
            electron: Some(e.completed as f64),
            pass: r.completed == r.runs && r.completed >= e.completed,
        },
        time("launch to first screen (ms)", r.launch_ms, e.launch_ms),
        time("scroll frame median (ms)", r.frame_median_ms, e.frame_median_ms),
        le("long frames", r.long_frames as f64, e.long_frames as f64),
        time("typing median (ms)", r.type_median_ms, e.type_median_ms),
        time("typing p95 (ms)", r.type_p95_ms, e.type_p95_ms),
        time("JSON round trip (ms)", r.json_ms, e.json_ms),
        time("sort with localeCompare (ms)", r.sort_ms, e.sort_ms),
        time("plain loop (ms)", r.loop_ms, e.loop_ms),
        GateRow {
            name: "memory (MB)",
            rustron: r.mem_mb,
            electron: e.mem_mb,
            pass: matches!((r.mem_mb, e.mem_mb), (Some(a), Some(b)) if a < b),
        },
        le("JS errors", r.errors as f64, e.errors as f64),
    ]
}

pub fn render_markdown(rows: &[GateRow], header: &str) -> String {
    let fmt = |v: Option<f64>| v.map_or("missing".to_string(), |x| format!("{x:.1}"));
    let mut s = format!("{header}\n\n| Measure | Rustron | Electron | Result |\n|---|---|---|---|\n");
    for row in rows {
        s.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            row.name,
            fmt(row.rustron),
            fmt(row.electron),
            if row.pass { "PASS" } else { "FAIL" }
        ));
    }
    s
}
