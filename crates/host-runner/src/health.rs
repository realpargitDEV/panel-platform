//! Asking a running project whether it is actually working.
//!
//! A process that exists is not a project that works. It can be up and wedged,
//! up and still starting, or up and listening on the wrong interface. Docker
//! answers this with the health check baked into the image; on the host there is
//! nothing to bake it into, so it is asked from here.
//!
//! The four kinds are the four the schema allows: `NONE`, `HTTP`, `TCP` and
//! `COMMAND`.
//!
//! # Why HTTP is spoken directly rather than through a client library
//!
//! A health target is a port on this machine that this application just started.
//! It is `http://127.0.0.1:<port>/...` and it is never TLS, never redirected,
//! never authenticated and never cross-origin. What is needed from a response is
//! its status code and nothing else — not its body, not its headers, not its
//! encoding. A request line and a read of the status line covers it in a few
//! lines that can be tested against a listener in the same process.
//!
//! An `https://` target is therefore **refused rather than attempted**, with a
//! message saying so. Quietly failing every check against a target the user
//! believed was being polled would be the worse answer.

use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// What a check said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Health {
    /// No check is configured. Distinct from passing: nothing was asked.
    None,
    Passing,
    /// Failed, with the reason worth showing.
    Failing(String),
}

/// A check, as the runtime row describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// `NONE`, `HTTP`, `TCP` or `COMMAND`.
    pub kind: String,
    /// A URL for `HTTP`, a port for `TCP`, a command line for `COMMAND`.
    pub target: Option<String>,
    pub timeout: Duration,
}

impl Check {
    /// Read one from the stored strings, substituting the allocated port.
    ///
    /// `{port}` in a target is replaced, because the port is not known when the
    /// runtime row is written — it is allocated per project, and a template that
    /// hard-coded one would be wrong for every project but the first.
    pub fn resolved(kind: &str, target: Option<&str>, timeout_s: i64, port: Option<u16>) -> Self {
        let target = target.map(|value| match port {
            Some(port) => value.replace("{port}", &port.to_string()),
            None => value.to_string(),
        });
        Self {
            kind: kind.to_string(),
            target,
            // A zero or negative timeout in the row would otherwise mean every
            // check fails instantly.
            timeout: Duration::from_secs(timeout_s.clamp(1, 300).unsigned_abs()),
        }
    }
}

/// Run one check, once.
pub async fn check(check: &Check) -> Health {
    match check.kind.as_str() {
        "HTTP" => match &check.target {
            Some(target) => http(target, check.timeout).await,
            None => Health::Failing("the health check has no URL".to_string()),
        },
        "TCP" => match &check.target {
            Some(target) => tcp(target, check.timeout).await,
            None => Health::Failing("the health check has no port".to_string()),
        },
        "COMMAND" => match &check.target {
            Some(target) => command(target, check.timeout).await,
            None => Health::Failing("the health check has no command".to_string()),
        },
        // NONE, and anything a future schema adds that this build does not know.
        // Reporting "nothing was asked" is honest; reporting failure would mark
        // a working project unhealthy on an upgrade.
        _ => Health::None,
    }
}

/// GET the target, and read only the status line. 2xx and 3xx pass.
async fn http(target: &str, timeout: Duration) -> Health {
    if target.starts_with("https://") {
        return Health::Failing(
            "https health checks are not supported; use http against the local port".to_string(),
        );
    }

    let Some((authority, path)) = split_url(target) else {
        return Health::Failing(format!("`{target}` is not a URL this can request"));
    };

    match tokio::time::timeout(timeout, http_status(&authority, &path)).await {
        Err(_) => Health::Failing(format!("no response within {}s", timeout.as_secs())),
        Ok(Err(error)) => Health::Failing(error),
        Ok(Ok(status)) if (200..400).contains(&status) => Health::Passing,
        Ok(Ok(status)) => Health::Failing(format!("responded {status}")),
    }
}

/// Split `http://host:port/path` into its authority and its path.
fn split_url(target: &str) -> Option<(String, String)> {
    let rest = target.strip_prefix("http://")?;
    match rest.find('/') {
        Some(index) => {
            let (authority, path) = rest.split_at(index);
            (!authority.is_empty()).then(|| (authority.to_string(), path.to_string()))
        }
        None => (!rest.is_empty()).then(|| (rest.to_string(), "/".to_string())),
    }
}

/// Send a request and read the status code out of the first line.
async fn http_status(authority: &str, path: &str) -> Result<u16, String> {
    let mut stream = TcpStream::connect(authority)
        .await
        .map_err(|error| format!("could not connect: {error}"))?;

    // HTTP/1.1 requires Host. `Connection: close` so the server ends the
    // response rather than holding the socket open for a reuse that will never
    // come.
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\nAccept: */*\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|error| format!("could not send the request: {error}"))?;

    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .await
        .map_err(|error| format!("could not read the response: {error}"))?;

    // `HTTP/1.1 200 OK` — the code is the second word.
    line.split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| format!("the response did not start with a status line: {line:?}"))
}

/// Connecting is the whole check.
async fn tcp(target: &str, timeout: Duration) -> Health {
    // A bare port is the common case; `host:port` is allowed for a project that
    // binds somewhere specific.
    let authority = if target.contains(':') {
        target.to_string()
    } else {
        format!("127.0.0.1:{target}")
    };

    match tokio::time::timeout(timeout, TcpStream::connect(&authority)).await {
        Err(_) => Health::Failing(format!("no connection within {}s", timeout.as_secs())),
        Ok(Err(error)) => Health::Failing(format!("could not connect to {authority}: {error}")),
        Ok(Ok(_)) => Health::Passing,
    }
}

/// Exit zero is healthy.
async fn command(target: &str, timeout: Duration) -> Health {
    let Ok(mut words) = crate::command::split_command(target) else {
        return Health::Failing(format!("`{target}` could not be read as a command"));
    };
    if words.is_empty() {
        return Health::Failing("the health check command is empty".to_string());
    }
    let program = words.remove(0);

    let mut process = tokio::process::Command::new(&program);
    process
        .args(&words)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // So a health check that spawns something does not leak it once per
    // interval, forever.
    project_host_platform::as_group_leader(&mut process);

    let Ok(mut child) = process.spawn() else {
        return Health::Failing(format!("could not run `{program}`"));
    };
    let pid = child.id();

    match tokio::time::timeout(timeout, child.wait()).await {
        Err(_) => {
            if let Some(pid) = pid {
                let _ = project_host_platform::kill_tree(pid).await;
            }
            Health::Failing(format!(
                "`{program}` did not finish within {}s",
                timeout.as_secs()
            ))
        }
        Ok(Err(error)) => Health::Failing(format!("`{program}` could not be waited on: {error}")),
        Ok(Ok(status)) if status.success() => Health::Passing,
        Ok(Ok(status)) => Health::Failing(format!(
            "`{program}` exited {}",
            status.code().unwrap_or(-1)
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn a_check_with_no_kind_asks_nothing() {
        let check = Check::resolved("NONE", None, 5, None);
        assert_eq!(check.kind, "NONE");
    }

    #[tokio::test]
    async fn no_check_is_not_the_same_as_a_passing_one() {
        assert_eq!(
            check(&Check::resolved("NONE", None, 5, None)).await,
            Health::None
        );
    }

    /// A kind from a future schema must not mark a working project unhealthy.
    #[tokio::test]
    async fn an_unknown_kind_reports_that_nothing_was_asked() {
        assert_eq!(
            check(&Check::resolved("GRPC", Some("whatever"), 5, None)).await,
            Health::None
        );
    }

    #[test]
    fn the_allocated_port_is_substituted_into_the_target() {
        let check = Check::resolved(
            "HTTP",
            Some("http://127.0.0.1:{port}/healthz"),
            5,
            Some(8081),
        );
        assert_eq!(
            check.target.as_deref(),
            Some("http://127.0.0.1:8081/healthz")
        );
    }

    #[test]
    fn a_nonsense_timeout_becomes_a_usable_one() {
        // Zero would otherwise mean every check fails before it is sent.
        assert_eq!(
            Check::resolved("TCP", None, 0, None).timeout,
            Duration::from_secs(1)
        );
        assert_eq!(
            Check::resolved("TCP", None, -9, None).timeout,
            Duration::from_secs(1)
        );
        assert_eq!(
            Check::resolved("TCP", None, 9_999, None).timeout,
            Duration::from_secs(300)
        );
    }

    #[test]
    fn urls_split_into_an_authority_and_a_path() {
        assert_eq!(
            split_url("http://127.0.0.1:8080/healthz"),
            Some(("127.0.0.1:8080".to_string(), "/healthz".to_string()))
        );
        assert_eq!(
            split_url("http://127.0.0.1:8080"),
            Some(("127.0.0.1:8080".to_string(), "/".to_string()))
        );
        assert_eq!(split_url("127.0.0.1:8080"), None);
    }

    #[tokio::test]
    async fn tcp_passes_against_something_listening_and_fails_against_nothing() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();

        assert_eq!(
            check(&Check::resolved("TCP", Some(&port.to_string()), 2, None)).await,
            Health::Passing
        );

        drop(listener);
        // Port 1 is reserved and nothing will be listening on it.
        assert!(matches!(
            check(&Check::resolved("TCP", Some("1"), 1, None)).await,
            Health::Failing(_)
        ));
    }

    /// Serve one canned response, then close.
    async fn serve_once(status_line: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();

        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut discard = String::new();
                let _ = BufReader::new(&mut stream).read_line(&mut discard).await;
                let response =
                    format!("{status_line}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.flush().await;
            }
        });

        port
    }

    #[tokio::test]
    async fn a_two_hundred_passes() {
        let port = serve_once("HTTP/1.1 200 OK").await;
        let target = format!("http://127.0.0.1:{port}/healthz");
        assert_eq!(
            check(&Check::resolved("HTTP", Some(&target), 5, None)).await,
            Health::Passing
        );
    }

    /// A redirect is a server that is up and answering, which is what the check
    /// is asking about.
    #[tokio::test]
    async fn a_redirect_passes() {
        let port = serve_once("HTTP/1.1 302 Found").await;
        let target = format!("http://127.0.0.1:{port}/");
        assert_eq!(
            check(&Check::resolved("HTTP", Some(&target), 5, None)).await,
            Health::Passing
        );
    }

    #[tokio::test]
    async fn a_server_error_fails_and_says_what_it_answered() {
        let port = serve_once("HTTP/1.1 503 Service Unavailable").await;
        let target = format!("http://127.0.0.1:{port}/");
        match check(&Check::resolved("HTTP", Some(&target), 5, None)).await {
            Health::Failing(reason) => assert!(reason.contains("503"), "reason was {reason:?}"),
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn nothing_listening_fails_rather_than_hanging() {
        let target = "http://127.0.0.1:1/".to_string();
        assert!(matches!(
            check(&Check::resolved("HTTP", Some(&target), 2, None)).await,
            Health::Failing(_)
        ));
    }

    /// Refused rather than attempted, so a user who configured TLS is told
    /// instead of watching every check fail for no stated reason.
    #[tokio::test]
    async fn an_https_target_is_refused_with_a_reason() {
        match check(&Check::resolved(
            "HTTP",
            Some("https://example.com/"),
            5,
            None,
        ))
        .await
        {
            Health::Failing(reason) => assert!(reason.contains("https"), "reason was {reason:?}"),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_command_check_passes_on_zero_and_fails_otherwise() {
        #[cfg(windows)]
        let (good, bad) = ("cmd /C exit 0", "cmd /C exit 7");
        #[cfg(unix)]
        let (good, bad) = ("sh -c \"exit 0\"", "sh -c \"exit 7\"");

        assert_eq!(
            check(&Check::resolved("COMMAND", Some(good), 10, None)).await,
            Health::Passing
        );
        match check(&Check::resolved("COMMAND", Some(bad), 10, None)).await {
            Health::Failing(reason) => assert!(reason.contains('7'), "reason was {reason:?}"),
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_command_that_does_not_exist_fails_rather_than_panicking() {
        assert!(matches!(
            check(&Check::resolved(
                "COMMAND",
                Some("definitely-not-real-xyz"),
                5,
                None
            ))
            .await,
            Health::Failing(_)
        ));
    }
}
