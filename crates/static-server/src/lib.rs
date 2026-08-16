//! Serving a static site, so that a static site does not need a container.
//!
//! # Why this exists
//!
//! Every other runtime the product offers brings its own way of running: a Node
//! project runs `node`, a Python project runs `python`. A static site has no
//! toolchain to find — it is a directory of files, and something has to serve
//! them. Under Docker that something was the image's web server. Without
//! Docker, a static project could not start at all, and the message it gave was
//! "host mode cannot run STATIC projects yet; run this one in Docker" — a
//! Docker instruction in a product that no longer requires Docker, for the one
//! runtime a beginner is most likely to pick first.
//!
//! Reaching for whatever the machine happens to have — `python -m http.server`,
//! `npx serve` — was rejected: it makes a static site the *only* runtime whose
//! ability to start depends on a language the project does not use.
//!
//! # What it is
//!
//! A read-only HTTP/1.1 server for `GET` and `HEAD` under one directory. It is
//! deliberately small, and everything it refuses is refused because there is no
//! reason for a static site preview to do it:
//!
//! * No writes, no uploads, no methods other than `GET` and `HEAD`.
//! * No directory listings. A request for a directory serves its `index.html`
//!   or answers 404; enumerating a folder is a way to expose files the author
//!   did not mean to publish.
//! * Loopback only, by the caller's choice of address. The default binds
//!   `127.0.0.1`, so a static preview is not a file server for the network the
//!   machine happens to be on.
//!
//! # Path safety
//!
//! [`resolve`] is the whole security boundary and is a pure function, so it is
//! tested against the traversal attempts that matter without a socket. A path
//! is decoded, split on `/`, and rebuilt component by component: `..` pops,
//! anything absolute or with a drive letter is refused, and the result is
//! required to still be inside the root. Nothing is passed to the filesystem
//! until it has survived all of that.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// How long a connection may take to send its request line and headers.
///
/// Without it, a client that opens a socket and says nothing holds a task
/// forever, and enough of them hold all of them.
const HEADER_TIMEOUT: Duration = Duration::from_secs(10);

/// The largest request head this will read.
///
/// A static file server has no use for a large one, and a bound is what stops a
/// client from growing the buffer until the process runs out of memory.
const MAX_HEAD_BYTES: usize = 16 * 1024;

/// Serve `root` on `address` until the process ends.
///
/// Returns only on a failure to bind, which is the case the caller has to
/// report: a port that cannot be bound is the difference between a project that
/// started and one that did not.
pub async fn serve(root: PathBuf, address: &str) -> std::io::Result<()> {
    let root = root.canonicalize().unwrap_or(root);
    let listener = TcpListener::bind(address).await?;

    // On stdout, so it lands in the project's console like any other project's
    // startup line. This is the line a user looks for to know it is up.
    println!("serving {} on http://{address}", root.display());

    loop {
        let (stream, _peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                // One refused connection is not a reason to stop serving.
                eprintln!("could not accept a connection: {error}");
                continue;
            }
        };

        let root = root.clone();
        tokio::spawn(async move {
            if let Err(error) = handle(stream, &root).await {
                // Ordinary: a browser closing a tab mid-response lands here.
                tracing::debug!(%error, "a connection ended early");
            }
        });
    }
}

/// Read one request and answer it.
///
/// Connection-per-request: `Connection: close` is sent and the socket is shut
/// down afterwards. Keep-alive would be faster and is not worth the state
/// machine for a preview server.
async fn handle(stream: TcpStream, root: &Path) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream);

    let request = match tokio::time::timeout(HEADER_TIMEOUT, read_head(&mut reader)).await {
        Ok(Ok(Some(request))) => request,
        // Nothing readable, or nothing sent in time. Neither deserves a reply.
        _ => return Ok(()),
    };

    let response = respond(root, &request).await;
    let stream = reader.get_mut();
    stream.write_all(&response).await?;
    stream.flush().await?;
    stream.shutdown().await
}

/// The parts of a request this server uses.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Request {
    method: String,
    target: String,
}

/// Read the request line, and discard the headers.
///
/// The headers are read rather than ignored because they have to be drained
/// off the socket before the reply, and bounded because a client that never
/// sends a blank line would otherwise be read forever.
async fn read_head<R>(reader: &mut BufReader<R>) -> std::io::Result<Option<Request>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut line = String::new();
    if reader.read_line(&mut line).await? == 0 {
        return Ok(None);
    }

    let mut fields = line.split_whitespace();
    let (Some(method), Some(target)) = (fields.next(), fields.next()) else {
        return Ok(None);
    };
    let request = Request {
        method: method.to_string(),
        target: target.to_string(),
    };

    let mut read = line.len();
    loop {
        let mut header = String::new();
        let count = reader.read_line(&mut header).await?;
        read += count;
        if count == 0 || header == "\r\n" || header == "\n" || read > MAX_HEAD_BYTES {
            break;
        }
    }

    Ok(Some(request))
}

/// Build the whole response for one request.
///
/// Separated from the socket so every status this can produce is testable
/// without one.
async fn respond(root: &Path, request: &Request) -> Vec<u8> {
    if request.method != "GET" && request.method != "HEAD" {
        return error_response(405, "Method Not Allowed", "Only GET and HEAD are served.");
    }

    let Some(path) = resolve(root, &request.target) else {
        return error_response(403, "Forbidden", "That path is outside the site.");
    };

    let path = match tokio::fs::metadata(&path).await {
        // A directory is served as its index, never as a listing.
        Ok(meta) if meta.is_dir() => path.join("index.html"),
        Ok(_) => path,
        Err(_) => return not_found(),
    };

    let Ok(body) = tokio::fs::read(&path).await else {
        return not_found();
    };

    let mut response = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: {}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-cache\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Connection: close\r\n\r\n",
        content_type(&path),
        body.len()
    )
    .into_bytes();

    // A HEAD carries the headers of the GET and none of the body, which is what
    // makes it useful as a health check.
    if request.method == "GET" {
        response.extend_from_slice(&body);
    }
    response
}

fn not_found() -> Vec<u8> {
    error_response(404, "Not Found", "There is no such file in this site.")
}

fn error_response(code: u16, reason: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {code} {reason}\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

/// Turn a request target into a path inside `root`, or refuse it.
///
/// The security boundary of this crate, and a pure function so that every way
/// out of the directory can be tested without a socket or a filesystem.
///
/// `None` means the target tried to leave the root. It is deliberately not
/// "clamp to the root and serve something": a request for `/../../secrets` is
/// not a request for the index, and answering it with one would hide an attempt
/// that the 403 makes visible.
fn resolve(root: &Path, target: &str) -> Option<PathBuf> {
    // The query string and fragment are not part of the path.
    let path = target
        .split(['?', '#'])
        .next()
        .unwrap_or(target)
        .trim_start_matches('/');

    let decoded = percent_decode(path)?;

    let mut resolved = root.to_path_buf();
    let mut depth = 0usize;

    for segment in decoded.split('/') {
        match segment {
            "" | "." => continue,
            ".." => {
                // Climbing above the root is the attack. Refused rather than
                // clamped, so it cannot be done by prefixing enough of them.
                depth = depth.checked_sub(1)?;
                resolved.pop();
            }
            segment => {
                // A segment that is anything other than a plain name — a root,
                // a drive prefix, a bare `..` that survived decoding — must not
                // be joined, because `join` with an absolute path *replaces*
                // the whole buffer rather than extending it.
                let mut components = Path::new(segment).components();
                match (components.next(), components.next()) {
                    (Some(Component::Normal(name)), None) => {
                        resolved.push(name);
                        depth += 1;
                    }
                    _ => return None,
                }
            }
        }
    }

    Some(resolved)
}

/// Decode `%XX` escapes. `None` for a malformed escape or an embedded NUL.
///
/// Written out rather than pulled in: this is the only encoding this crate
/// deals with, and a dependency for twenty lines would be a dependency to audit
/// for twenty lines.
fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        match bytes.get(index) {
            Some(b'%') => {
                let high = bytes.get(index + 1)?;
                let low = bytes.get(index + 2)?;
                let byte = (hex(*high)? << 4) | hex(*low)?;
                // A NUL truncates a path at the operating-system boundary,
                // which is a classic way past a check like this one.
                if byte == 0 {
                    return None;
                }
                out.push(byte);
                index += 3;
            }
            Some(byte) => {
                out.push(*byte);
                index += 1;
            }
            None => break,
        }
    }

    String::from_utf8(out).ok()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// The `Content-Type` for a file, by extension.
///
/// A short table rather than a sniffing library. Everything unrecognised is
/// `application/octet-stream`, which a browser downloads rather than executes —
/// the safe direction for a guess.
fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "txt" | "md" => "text/plain; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "pdf" => "application/pdf",
        "wasm" => "application/wasm",
        "map" => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from(if cfg!(windows) {
            r"C:\site"
        } else {
            "/srv/site"
        })
    }

    #[test]
    fn an_ordinary_path_resolves_inside_the_root() {
        let resolved = resolve(&root(), "/assets/app.css").expect("a path");
        assert!(resolved.starts_with(root()));
        assert!(resolved.ends_with("assets/app.css") || resolved.ends_with(r"assets\app.css"));
    }

    #[test]
    fn the_root_itself_resolves_to_the_root() {
        assert_eq!(resolve(&root(), "/").expect("a path"), root());
        assert_eq!(resolve(&root(), "").expect("a path"), root());
    }

    /// The whole reason this function exists. Every one of these is a real
    /// attempt seen against real static servers.
    #[test]
    fn nothing_escapes_the_root() {
        for target in [
            "/../secrets.env",
            "/../../etc/passwd",
            "/assets/../../secrets.env",
            "/./../../secrets.env",
            // Percent-encoded, which is what defeats a check done before
            // decoding.
            "/%2e%2e/secrets.env",
            "/%2e%2e%2f%2e%2e%2fetc%2fpasswd",
            // A NUL, which truncates the path at the OS boundary.
            "/index.html%00.txt",
        ] {
            assert_eq!(resolve(&root(), target), None, "{target} escaped the root");
        }
    }

    /// Climbing back into the root after leaving it is still leaving it. A
    /// depth counter rather than a final `starts_with` check, because the
    /// latter passes for `/../site/secrets` when two roots share a parent.
    #[test]
    fn climbing_out_and_back_in_is_still_refused() {
        assert_eq!(resolve(&root(), "/../site/index.html"), None);
        // …but climbing within the root is fine, because it never leaves.
        assert_eq!(
            resolve(&root(), "/assets/../index.html").expect("a path"),
            root().join("index.html")
        );
    }

    /// `join` with an absolute path replaces the buffer instead of extending
    /// it, so an absolute segment would silently serve the whole filesystem.
    #[test]
    fn an_absolute_segment_cannot_replace_the_root() {
        for target in ["//etc/passwd", "/C:/Windows/win.ini", "/%2fetc%2fpasswd"] {
            let resolved = resolve(&root(), target);
            if let Some(path) = resolved {
                assert!(
                    path.starts_with(root()),
                    "{target} resolved to {} outside the root",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn a_query_string_is_not_part_of_the_path() {
        assert_eq!(
            resolve(&root(), "/index.html?v=2#top").expect("a path"),
            root().join("index.html")
        );
    }

    #[test]
    fn a_malformed_escape_is_refused_rather_than_guessed_at() {
        assert_eq!(percent_decode("%zz"), None);
        assert_eq!(percent_decode("%2"), None);
        assert_eq!(percent_decode("a%20b").as_deref(), Some("a b"));
    }

    #[tokio::test]
    async fn a_file_is_served_with_its_type_and_length() {
        let directory = tempfile::tempdir().expect("temp dir");
        std::fs::write(directory.path().join("index.html"), "<h1>hello</h1>").expect("write");

        let response = respond(
            directory.path(),
            &Request {
                method: "GET".to_string(),
                target: "/index.html".to_string(),
            },
        )
        .await;

        let text = String::from_utf8_lossy(&response);
        assert!(text.starts_with("HTTP/1.1 200 OK"), "{text}");
        assert!(
            text.contains("Content-Type: text/html; charset=utf-8"),
            "{text}"
        );
        assert!(text.contains("Content-Length: 14"), "{text}");
        assert!(text.ends_with("<h1>hello</h1>"), "{text}");
    }

    /// A request for a directory serves its index. Not a listing: enumerating a
    /// folder exposes files the author did not mean to publish.
    #[tokio::test]
    async fn a_directory_is_served_as_its_index_and_never_as_a_listing() {
        let directory = tempfile::tempdir().expect("temp dir");
        std::fs::write(directory.path().join("index.html"), "root index").expect("write");
        std::fs::create_dir(directory.path().join("docs")).expect("mkdir");
        std::fs::write(directory.path().join("docs/secret-draft.md"), "x").expect("write");

        let response = respond(
            directory.path(),
            &Request {
                method: "GET".to_string(),
                target: "/".to_string(),
            },
        )
        .await;
        assert!(String::from_utf8_lossy(&response).ends_with("root index"));

        // `docs` has no index, so it is a 404 rather than a list of its files.
        let response = respond(
            directory.path(),
            &Request {
                method: "GET".to_string(),
                target: "/docs/".to_string(),
            },
        )
        .await;
        let text = String::from_utf8_lossy(&response);
        assert!(text.starts_with("HTTP/1.1 404"), "{text}");
        assert!(
            !text.contains("secret-draft"),
            "a directory listing leaked: {text}"
        );
    }

    /// HEAD is what a TCP or HTTP health check uses, so it has to answer with
    /// the headers and no body.
    #[tokio::test]
    async fn head_answers_with_headers_and_no_body() {
        let directory = tempfile::tempdir().expect("temp dir");
        std::fs::write(directory.path().join("index.html"), "<h1>hello</h1>").expect("write");

        let response = respond(
            directory.path(),
            &Request {
                method: "HEAD".to_string(),
                target: "/index.html".to_string(),
            },
        )
        .await;

        let text = String::from_utf8_lossy(&response);
        assert!(text.starts_with("HTTP/1.1 200 OK"), "{text}");
        assert!(text.contains("Content-Length: 14"), "{text}");
        assert!(text.ends_with("\r\n\r\n"), "a HEAD carried a body: {text}");
    }

    #[tokio::test]
    async fn a_write_method_is_refused() {
        let directory = tempfile::tempdir().expect("temp dir");
        for method in ["POST", "PUT", "DELETE", "PATCH"] {
            let response = respond(
                directory.path(),
                &Request {
                    method: method.to_string(),
                    target: "/".to_string(),
                },
            )
            .await;
            assert!(
                String::from_utf8_lossy(&response).starts_with("HTTP/1.1 405"),
                "{method} was not refused"
            );
        }
    }

    /// A traversal attempt is a 403, not a 404. The difference matters: a 404
    /// would hide that somebody tried.
    #[tokio::test]
    async fn a_traversal_attempt_is_refused_rather_than_reported_as_missing() {
        let directory = tempfile::tempdir().expect("temp dir");
        let response = respond(
            directory.path(),
            &Request {
                method: "GET".to_string(),
                target: "/../../etc/passwd".to_string(),
            },
        )
        .await;
        assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 403"));
    }

    #[test]
    fn types_are_given_for_what_a_site_is_made_of() {
        for (name, expected) in [
            ("index.html", "text/html; charset=utf-8"),
            ("app.CSS", "text/css; charset=utf-8"),
            ("bundle.js", "text/javascript; charset=utf-8"),
            ("logo.svg", "image/svg+xml"),
            ("photo.jpeg", "image/jpeg"),
            ("font.woff2", "font/woff2"),
        ] {
            assert_eq!(content_type(Path::new(name)), expected, "for {name}");
        }

        // Unrecognised downloads rather than executes, which is the safe way
        // for a guess to be wrong.
        assert_eq!(
            content_type(Path::new("thing.unknown")),
            "application/octet-stream"
        );
    }

    /// End to end over a real socket: the one thing the pure tests cannot
    /// cover is that the pieces are wired to each other.
    #[tokio::test]
    async fn a_real_request_over_a_real_socket_is_answered() {
        use tokio::io::AsyncReadExt;

        let directory = tempfile::tempdir().expect("temp dir");
        std::fs::write(directory.path().join("index.html"), "<p>served</p>").expect("write");

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let root = directory.path().to_path_buf();

        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                let _ = handle(stream, &root).await;
            }
        });

        let mut client = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect");
        client
            .write_all(b"GET /index.html HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .expect("write");

        let mut received = String::new();
        client.read_to_string(&mut received).await.expect("read");

        assert!(received.starts_with("HTTP/1.1 200 OK"), "{received}");
        assert!(received.ends_with("<p>served</p>"), "{received}");
    }
}
