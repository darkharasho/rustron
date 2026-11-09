use rustron_bench::prepare::*;

#[test]
fn inject_before_first_script() {
    let out = inject_script("<head><script src=a.js></script></head>", "p.js");
    assert!(out.contains("<script src=\"p.js\"></script><script src=a.js>"), "{out}");
}

#[test]
fn inject_no_script_uses_head() {
    let out = inject_script("<head><title>x</title></head>", "p.js");
    assert_eq!(out, "<head><title>x</title><script src=\"p.js\"></script></head>");
}

#[test]
fn inject_bare_fragment_prepends() {
    assert_eq!(inject_script("<div></div>", "p.js"), "<script src=\"p.js\"></script><div></div>");
}

#[test]
fn run_dir_probe_after_last_script() {
    let fx = tempfile::tempdir().unwrap();
    std::fs::write(
        fx.path().join("index.html"),
        "<html><head><script src=\"./a.js\"></script><script type=\"module\" src=\"./b.js\"></script></head><body></body></html>",
    )
    .unwrap();
    std::fs::create_dir(fx.path().join("assets")).unwrap();
    std::fs::write(fx.path().join("assets/x.txt"), "x").unwrap();
    let run = prepare_run_dir(fx.path(), "// probe").unwrap();
    let html = std::fs::read_to_string(run.path().join("index.html")).unwrap();
    let probe = html.find("__rustron_probe.js").unwrap();
    assert!(probe > html.find("b.js").unwrap(), "{html}");
    assert_eq!(std::fs::read_to_string(run.path().join("__rustron_probe.js")).unwrap(), "// probe");
    assert!(run.path().join("assets/x.txt").exists());
    // original untouched
    assert!(!std::fs::read_to_string(fx.path().join("index.html")).unwrap().contains("__rustron_probe"));
}

const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: hml-art: https:";

#[test]
fn run_dir_csp_allows_app_and_report() {
    let html = format!("<meta http-equiv=\"Content-Security-Policy\"\n content=\"{CSP}\" />");
    let out = relax_csp(&html);
    let want = "default-src 'self' app:; script-src 'self' app:; style-src 'self' app: 'unsafe-inline'; img-src 'self' app: data: hml-art: https:; connect-src 'self' app: http://127.0.0.1:*";
    assert!(out.contains(&format!("content=\"{want}\"")), "{out}");
}

#[test]
fn csp_existing_connect_src_is_extended() {
    let out = relax_csp("<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'self'; connect-src 'self'; img-src 'self'\">");
    assert!(out.contains("connect-src 'self' app: http://127.0.0.1:*; img-src"), "{out}");
}

#[test]
fn no_csp_leaves_html_unchanged() {
    let html = "<html><head><script src=a.js></script></head></html>";
    assert_eq!(relax_csp(html), html);
}
