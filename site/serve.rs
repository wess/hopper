//! Minimal static server for the Hopper marketing site. No dependencies —
//! `rustc site/serve.rs -o /tmp/hoppersite && PORT=3000 /tmp/hoppersite`.
//!
//! Replaces the earlier Bun `serve.ts`; the repo ships no JavaScript tooling.
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Component, Path, PathBuf};

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}

/// Resolve a request path under `root`, refusing any traversal outside it.
fn resolve(root: &Path, request: &str) -> Option<PathBuf> {
    let raw = request.split('?').next().unwrap_or("/");
    let rel = if raw == "/" { "index.html" } else { raw.trim_start_matches('/') };
    let candidate = Path::new(rel);
    // A `..` component would escape the site directory; reject it outright.
    if candidate.components().any(|c| matches!(c, Component::ParentDir)) {
        return None;
    }
    Some(root.join(candidate))
}

fn main() {
    // Serve the directory this file lives in (or $SITE_DIR).
    let root = std::env::var("SITE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("site"));
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3000);

    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind");
    println!("serving {} on http://localhost:{port}", root.display());

    for stream in listener.incoming().flatten() {
        let mut stream = stream;
        let mut buf = [0u8; 2048];
        let Ok(n) = stream.read(&mut buf) else { continue };
        let request = String::from_utf8_lossy(&buf[..n]);
        let path = request.lines().next().and_then(|l| l.split(' ').nth(1)).unwrap_or("/");

        let (status, body, ctype): (&str, Vec<u8>, &str) = match resolve(&root, path)
            .filter(|p| p.is_file())
            .and_then(|p| std::fs::read(&p).ok().map(|b| (content_type(&p), b)))
        {
            Some((ctype, body)) => ("200 OK", body, ctype),
            None => ("404 Not Found", b"Not found".to_vec(), "text/plain"),
        };
        let header = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(&body);
    }
}
