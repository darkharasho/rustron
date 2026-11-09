use rustron_bench::stats::{median, p95};
use rustron_bench::*;

fn rep(frames: Vec<f64>, types: Vec<f64>) -> ProbeReport {
    ProbeReport {
        engine: "rustron".into(),
        grid_since_launch: 100.0,
        grid_since_nav: 50.0,
        json_ms: 1.0,
        sort_ms: 1.0,
        loop_ms: 1.0,
        frame_ms: frames,
        type_ms: types,
        errors: 0,
        fail: None,
    }
}

fn run(report: Option<ProbeReport>, timed_out: bool) -> RunResult {
    RunResult { engine: Engine::Rustron, report, mem_mb: Some(100.0), timed_out }
}

#[allow(clippy::too_many_arguments)]
fn sum(launch: f64, json: f64, sort: f64, lp: f64, fr: f64, tm: f64, tp: f64, mem: f64) -> Summary {
    Summary {
        runs: 6,
        completed: 6,
        launch_ms: Some(launch),
        json_ms: Some(json),
        sort_ms: Some(sort),
        loop_ms: Some(lp),
        frame_median_ms: Some(fr),
        long_frames: 0,
        type_median_ms: Some(tm),
        type_p95_ms: Some(tp),
        mem_mb: Some(mem),
        errors: 0,
    }
}

fn row<'a>(rows: &'a [GateRow], name: &str) -> &'a GateRow {
    rows.iter().find(|r| r.name == name).unwrap()
}

#[test]
fn median_odd_even() {
    assert_eq!(median(&[3., 1., 2.]), Some(2.0));
    assert_eq!(median(&[1., 2., 3., 4.]), Some(2.5));
    assert_eq!(median(&[]), None);
}

#[test]
fn p95_nearest_rank() {
    let xs: Vec<f64> = (1..=100).map(|x| x as f64).collect();
    assert_eq!(p95(&xs), Some(95.0));
    assert_eq!(p95(&[10.0]), Some(10.0));
    let ys: Vec<f64> = (1..=14).map(|x| x as f64).collect();
    assert_eq!(p95(&ys), Some(14.0));
    assert_eq!(p95(&[]), None);
}

#[test]
fn spike_numbers_reproduce_known_verdicts() {
    let r = sum(379.0, 57.0, 5.35, 64.0, 8.3, 11.9, 19.6, 500.0);
    let e = sum(544.0, 35.5, 2.1, 78.5, 8.3, 14.65, 16.0, 365.0);
    let rows = evaluate(&r, &e, 0.05);
    assert_eq!(rows.len(), 11);
    for n in [
        "launch to first screen (ms)",
        "scroll frame median (ms)",
        "long frames",
        "typing median (ms)",
        "plain loop (ms)",
        "completed runs",
        "JS errors",
    ] {
        assert!(row(&rows, n).pass, "{n} should pass");
    }
    for n in ["typing p95 (ms)", "JSON round trip (ms)", "sort with localeCompare (ms)", "memory (MB)"] {
        assert!(!row(&rows, n).pass, "{n} should fail");
    }
}

#[test]
fn tolerance_edges() {
    let e = sum(544.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 376.0);
    let mut r = e.clone();
    r.launch_ms = Some(571.0);
    assert!(row(&evaluate(&r, &e, 0.05), "launch to first screen (ms)").pass);
    r.launch_ms = Some(572.0);
    assert!(!row(&evaluate(&r, &e, 0.05), "launch to first screen (ms)").pass);
    r.mem_mb = Some(376.0);
    assert!(!row(&evaluate(&r, &e, 0.05), "memory (MB)").pass);
}

#[test]
fn failed_rustron_run_fails_gate() {
    let e = sum(1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0);
    let mut r = e.clone();
    r.completed = 5;
    assert!(!row(&evaluate(&r, &e, 0.05), "completed runs").pass);
}

#[test]
fn missing_typing_is_fail_not_panic() {
    let runs = vec![run(Some(rep(vec![8.0; 4], vec![])), false); 3];
    let s = summarize(&runs);
    assert_eq!(s.completed, 3);
    assert_eq!(s.type_median_ms, None);
    let rows = evaluate(&s, &s, 0.05);
    assert!(!row(&rows, "typing median (ms)").pass);
    assert!(!row(&rows, "typing p95 (ms)").pass);
    let md = render_markdown(&rows, "hdr");
    for line in md.lines().filter(|l| l.contains("typing")) {
        assert!(line.contains("missing"), "{line}");
        assert!(line.contains("FAIL"), "{line}");
    }
    assert!(md.starts_with("hdr\n\n| Measure | Rustron | Electron | Result |"));
}

#[test]
fn summarize_pools_and_counts() {
    let runs = vec![
        run(Some(rep(vec![8., 8., 8., 30.], vec![1.0])), false),
        run(Some(rep(vec![8., 8., 8., 8.], vec![3.0])), false),
        run(Some(rep(vec![8.0], vec![])), true),
    ];
    let s = summarize(&runs);
    assert_eq!((s.runs, s.completed), (3, 2));
    assert_eq!(s.long_frames, 1);
    assert_eq!(s.frame_median_ms, Some(8.0));
    assert_eq!(s.type_median_ms, Some(2.0));
}

#[test]
fn probe_report_json_keys() {
    let j = r#"{"engine":"electron","gridSinceLaunch":1.5,"gridSinceNav":2.5,"jsonMs":3.0,
        "sortMs":4.0,"loopMs":5.0,"frameMs":[8.0],"typeMs":[9.0],"errors":2,"fail":null}"#;
    let p: ProbeReport = serde_json::from_str(j).unwrap();
    assert_eq!(p.grid_since_launch, 1.5);
    assert_eq!(p.errors, 2);
    assert!(p.fail.is_none());
    let out = serde_json::to_value(&p).unwrap();
    assert!(out["fail"].is_null() && out.get("typeMs").is_some());
    assert_eq!(serde_json::to_string(&Engine::Rustron).unwrap(), "\"rustron\"");
}
