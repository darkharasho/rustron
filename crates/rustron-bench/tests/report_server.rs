use rustron_bench::report_server::ReportServer;
use std::time::Duration;

#[test]
fn receives_post_body() {
    let s = ReportServer::start().unwrap();
    let resp = ureq::post(&s.url()).send_string("{\"a\":1}").unwrap();
    assert_eq!(resp.header("access-control-allow-origin"), Some("*"));
    assert_eq!(s.wait_report(Duration::from_secs(5)), Some("{\"a\":1}".to_string()));
}

#[test]
fn times_out_without_post() {
    let s = ReportServer::start().unwrap();
    assert_eq!(s.wait_report(Duration::from_millis(300)), None);
}
