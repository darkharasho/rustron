//! Resolver for `app://app/<path>` loads: maps URLs onto files under a root directory.

use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;
use url::Url;

pub const SCHEME: &str = "app";
pub const HOST: &str = "app";

pub struct Response {
    pub status: u16,
    pub mime: &'static str,
    pub body: Vec<u8>,
}

impl Response {
    fn status(status: u16) -> Self {
        Response { status, mime: "text/plain; charset=utf-8", body: Vec::new() }
    }
}

pub struct AppProtocol {
    root: PathBuf,
}

impl AppProtocol {
    /// `root` is canonicalized; it must exist.
    pub fn new(root: &Path) -> std::io::Result<Self> {
        Ok(AppProtocol { root: root.canonicalize()? })
    }

    pub fn respond(&self, url: &Url) -> Response {
        if url.host_str() != Some(HOST) {
            return Response::status(404);
        }
        // `Url::path` excludes query and fragment.
        let decoded = percent_decode_str(url.path()).decode_utf8_lossy();
        if decoded.split(['/', '\\']).any(|seg| seg == "..") {
            return Response::status(403);
        }
        let rel = decoded.trim_start_matches('/');
        let rel = if rel.is_empty() { "index.html" } else { rel };
        let canonical = match self.root.join(rel).canonicalize() {
            Ok(p) => p,
            Err(_) => return Response::status(404),
        };
        if !canonical.starts_with(&self.root) {
            return Response::status(403);
        }
        match std::fs::read(&canonical) {
            Ok(body) => Response { status: 200, mime: mime_for(&canonical), body },
            Err(_) => Response::status(404),
        }
    }
}

pub fn mime_for(path: &Path) -> &'static str {
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("woff2") => "font/woff2",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}
