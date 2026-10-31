use super::*;

/// Minimal local HTTP stub; records `METHOD /path` per request.
fn spawn_stub() -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let thread_log = log.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let log = thread_log.clone();
            std::thread::spawn(move || {
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 512];
                while !buffer.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut chunk) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buffer.extend_from_slice(&chunk[..n]),
                    }
                }
                let text = String::from_utf8_lossy(&buffer).into_owned();
                let mut parts = text.lines().next().unwrap_or("").split_whitespace();
                let method = parts.next().unwrap_or("").to_string();
                let path = parts.next().unwrap_or("").to_string();
                log.lock().unwrap().push(format!("{method} {path}"));
                let status = match (method.as_str(), path.as_str()) {
                    (_, "/missing") => "404 Not Found",
                    (_, "/limited") => "429 Too Many Requests",
                    ("HEAD", "/head405") => "405 Method Not Allowed",
                    _ => "200 OK",
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
            });
        }
    });
    (base, log)
}

// Part 02 test 7
#[test]
fn external_links_are_checked_only_when_opted_in() {
    let (base, log) = spawn_stub();
    let readme = format!(
        "[a]({base}/ok) [b]({base}/missing) [c]({base}/limited) [d]({base}/head405) [e]({base}/ok#frag)\n"
    );
    let repo = init_repo(&[("README.md", readme.as_str())]);

    let off = evaluate_with_options(repo.path(), &EvaluateOptions::default());
    assert!(off.findings.is_empty());
    assert!(log.lock().unwrap().is_empty(), "no request when disabled");

    let on = evaluate_with_options(
        repo.path(),
        &EvaluateOptions {
            check_external: true,
            ..EvaluateOptions::default()
        },
    );
    assert_eq!(
        categories(&on),
        vec![
            LinkClass::ExternalRateLimited.category(),
            LinkClass::ExternalUnreachable.category()
        ]
    );
    assert_eq!(on.findings[0].evidence["blocking"], false);
    assert_eq!(on.findings[1].evidence["blocking"], true);
    assert_eq!(on.metric.blocking_findings, 1);
    let requests = log.lock().unwrap().clone();
    let count = |needle: &str| requests.iter().filter(|r| r.as_str() == needle).count();
    assert_eq!(count("HEAD /ok"), 1, "URLs are de-duplicated: {requests:?}");
    assert_eq!(count("HEAD /head405"), 1);
    assert_eq!(count("GET /head405"), 1, "GET fallback after 405");
    assert_eq!(count("GET /missing"), 0);
}
