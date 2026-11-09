//! Builds a throwaway copy of the fixture with the probe injected and the CSP relaxed.
use anyhow::{Context, Result};
use std::path::Path;

/// Adds `<script src="{src}"></script>` immediately before the first `<script` tag,
/// else before `</head>`, else at the start.
pub fn inject_script(html: &str, src: &str) -> String {
    let tag = format!("<script src=\"{src}\"></script>");
    let at = html.find("<script").or_else(|| html.find("</head>")).unwrap_or(0);
    format!("{}{}{}", &html[..at], tag, &html[at..])
}

/// Adds the probe tag after the last `</script>` (or falls back to `inject_script`).
fn inject_after_last_script(html: &str, src: &str) -> String {
    match html.rfind("</script>") {
        Some(i) => {
            let at = i + "</script>".len();
            format!("{}<script src=\"{src}\"></script>{}", &html[..at], &html[at..])
        }
        None => inject_script(html, src),
    }
}

/// Rewrites the CSP meta so `app:` content and the probe's report POST are allowed.
/// Html without a CSP meta is returned unchanged.
pub fn relax_csp(html: &str) -> String {
    let Some(meta_at) = html.find("Content-Security-Policy") else {
        return html.to_string();
    };
    let Some(c_rel) = html[meta_at..].find("content=\"") else {
        return html.to_string();
    };
    let start = meta_at + c_rel + "content=\"".len();
    let Some(len) = html[start..].find('"') else {
        return html.to_string();
    };
    let csp = &html[start..start + len];
    let mut csp = csp.replace("'self'", "'self' app:");
    if let Some(i) = csp.find("connect-src") {
        let end = csp[i..].find(';').map_or(csp.len(), |e| i + e);
        csp.insert_str(end, " http://127.0.0.1:*");
    } else {
        csp = format!("{}; connect-src 'self' app: http://127.0.0.1:*", csp.trim_end().trim_end_matches(';'));
    }
    format!("{}{}{}", &html[..start], csp, &html[start + len..])
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}

pub fn prepare_run_dir(fixture: &Path, probe_js: &str) -> Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    copy_dir(fixture, dir.path()).with_context(|| format!("copying fixture {}", fixture.display()))?;
    std::fs::write(dir.path().join("__rustron_probe.js"), probe_js)?;
    let index = dir.path().join("index.html");
    let html = std::fs::read_to_string(&index).with_context(|| format!("reading {}", index.display()))?;
    let html = inject_after_last_script(&relax_csp(&html), "./__rustron_probe.js");
    std::fs::write(&index, html)?;
    Ok(dir)
}
