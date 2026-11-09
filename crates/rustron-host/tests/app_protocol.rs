use rustron_host::app_protocol::{mime_for, AppProtocol};
use std::path::Path;
use url::Url;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/app");

fn proto() -> AppProtocol {
    AppProtocol::new(Path::new(ROOT)).unwrap()
}

fn get(url: &str) -> rustron_host::app_protocol::Response {
    proto().respond(&Url::parse(url).unwrap())
}

fn index_bytes() -> Vec<u8> {
    std::fs::read(Path::new(ROOT).join("index.html")).unwrap()
}

#[test]
fn root_serves_index() {
    let r = get("app://app/");
    assert_eq!(r.status, 200);
    assert_eq!(r.mime, "text/html; charset=utf-8");
    assert_eq!(r.body, index_bytes());
}

#[test]
fn query_and_fragment_ignored() {
    let r = get("app://app/index.html?t0=1&x=y#top");
    assert_eq!(r.status, 200);
    assert_eq!(r.body, index_bytes());
}

#[test]
fn percent_encoded_unicode_path() {
    let r = get("app://app/My%20Mods/caf%C3%A9.png");
    assert_eq!(r.status, 200);
    assert_eq!(r.mime, "image/png");
}

#[test]
fn mime_table() {
    let cases = [
        ("a.html", "text/html; charset=utf-8"),
        ("a.js", "text/javascript; charset=utf-8"),
        ("a.mjs", "text/javascript; charset=utf-8"),
        ("a.css", "text/css; charset=utf-8"),
        ("a.json", "application/json"),
        ("a.png", "image/png"),
        ("a.jpg", "image/jpeg"),
        ("a.jpeg", "image/jpeg"),
        ("a.webp", "image/webp"),
        ("a.svg", "image/svg+xml"),
        ("a.woff2", "font/woff2"),
        ("a.wasm", "application/wasm"),
        ("a.bin", "application/octet-stream"),
        ("noext", "application/octet-stream"),
    ];
    for (name, mime) in cases {
        assert_eq!(mime_for(Path::new(name)), mime, "{name}");
    }
}

#[test]
fn missing_file_404() {
    let r = get("app://app/nope.js");
    assert_eq!(r.status, 404);
    assert!(r.body.is_empty());
}

#[test]
fn dotdot_forbidden() {
    // The URL parser leaves an encoded backslash alone, so the resolver itself must reject it.
    assert_eq!(get("app://app/assets/..%5C..%5CCargo.toml").status, 403);
    assert_eq!(get("app://app/assets/%5c..%5c..%5cCargo.toml").status, 403);
}

#[test]
fn dotdot_collapsed_by_parser_cannot_escape() {
    // `Url::parse` resolves `..` and `%2e%2e` segments before the resolver sees them, so these
    // become `/Cargo.toml` under the root (absent in the fixture) and can never reach the parent.
    for u in ["app://app/../Cargo.toml", "app://app/assets/%2e%2e/%2e%2e/Cargo.toml"] {
        assert_eq!(get(u).status, 404, "{u}");
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

#[cfg(unix)]
#[test]
fn symlink_escape_forbidden() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("app");
    copy_dir(Path::new(ROOT), &root);
    let outside = tmp.path().join("secret.txt");
    std::fs::write(&outside, "secret").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link.txt")).unwrap();
    let p = AppProtocol::new(&root).unwrap();
    let r = p.respond(&Url::parse("app://app/link.txt").unwrap());
    assert_eq!(r.status, 403);
}

#[cfg(windows)]
#[test]
fn symlink_escape_forbidden() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("app");
    copy_dir(Path::new(ROOT), &root);
    let outside = tmp.path().join("secret.txt");
    std::fs::write(&outside, "secret").unwrap();
    if std::os::windows::fs::symlink_file(&outside, root.join("link.txt")).is_err() {
        eprintln!("skipping: creating symlinks needs privilege");
        return;
    }
    let p = AppProtocol::new(&root).unwrap();
    let r = p.respond(&Url::parse("app://app/link.txt").unwrap());
    assert_eq!(r.status, 403);
}

#[test]
fn wrong_host_404() {
    assert_eq!(get("app://other/index.html").status, 404);
}
