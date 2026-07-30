//! Fetching bytes, with every hop checked and every limit enforced.
//!
//! Redirects are followed by hand rather than by the HTTP agent, because an
//! allowlist that is applied only to the URL you started with does not survive
//! the first `302`.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::net::{AllowedHost, NetError};

/// Larger than any artefact the project builds — the AppImage, its biggest, is
/// 89 MB — and small enough that a redirect to something enormous stops early
/// instead of filling the disk.
pub const MAX_BYTES: u64 = 256 * 1024 * 1024;

const MAX_REDIRECTS: usize = 5;

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error(transparent)]
    Forbidden(#[from] NetError),
    #[error("the download was redirected too many times")]
    TooManyRedirects,
    #[error("a redirect gave no destination")]
    RedirectWithoutLocation,
    #[error("the download is larger than this installer will accept")]
    TooLarge,
    #[error("the download ended early: {got} bytes of {expected}")]
    Truncated { got: u64, expected: u64 },
    #[error("cancelled")]
    Cancelled,
    #[error("the download failed: {0}")]
    Failed(String),
    #[error("the server answered {0}")]
    Http(u16),
}

/// Reports bytes fetched so far and the total expected, for a progress bar.
pub type Progress<'a> = &'a mut dyn FnMut(u64, u64);

/// Downloads into memory.
///
/// Memory rather than a file because the bytes must be verified before they are
/// allowed to exist anywhere a user might run them by accident, and the cap
/// above bounds what that costs.
pub fn fetch(
    agent: &ureq::Agent,
    url: &str,
    expected: u64,
    cancel: &AtomicBool,
    progress: Progress<'_>,
) -> Result<Vec<u8>, DownloadError> {
    if expected > MAX_BYTES {
        return Err(DownloadError::TooLarge);
    }

    let response = get_following_redirects(agent, url)?;

    let total = response
        .header("content-length")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(expected);

    if total > MAX_BYTES {
        return Err(DownloadError::TooLarge);
    }

    let mut reader = response.into_reader();
    let mut body = Vec::with_capacity(usize::try_from(total.min(MAX_BYTES)).unwrap_or(0));
    let mut buffer = [0u8; 64 * 1024];

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(DownloadError::Cancelled);
        }

        let read = reader
            .read(&mut buffer)
            .map_err(|error| DownloadError::Failed(error.to_string()))?;
        if read == 0 {
            break;
        }

        // Enforced against what actually arrives, not only against what the
        // server claimed in a header it is free to lie about.
        if body.len() as u64 + read as u64 > MAX_BYTES {
            return Err(DownloadError::TooLarge);
        }

        body.extend_from_slice(buffer.get(..read).unwrap_or_default());
        progress(body.len() as u64, total);
    }

    // A truncated installer would otherwise fail verification with a checksum
    // error, which would send a reader looking for tampering rather than for
    // the dropped connection that actually happened.
    if expected > 0 && (body.len() as u64) < expected {
        return Err(DownloadError::Truncated {
            got: body.len() as u64,
            expected,
        });
    }

    Ok(body)
}

/// Small companions to the artefact: the signature and `SHA256SUMS.txt`.
pub fn fetch_text(agent: &ureq::Agent, url: &str) -> Result<String, DownloadError> {
    let quiet = AtomicBool::new(false);
    let bytes = fetch(agent, url, 0, &quiet, &mut |_, _| {})?;

    String::from_utf8(bytes).map_err(|_| DownloadError::Failed("not text".to_owned()))
}

fn get_following_redirects(
    agent: &ureq::Agent,
    url: &str,
) -> Result<ureq::Response, DownloadError> {
    let mut url = url.to_owned();

    for _ in 0..=MAX_REDIRECTS {
        // Checked before every hop, including the first, so a redirect cannot
        // walk the download off GitHub one step at a time.
        AllowedHost::check(&url)?;

        let response = match agent.get(&url).call() {
            Ok(response) => response,
            Err(ureq::Error::Status(status, response)) if is_redirect(status) => response,
            Err(ureq::Error::Status(status, _)) => return Err(DownloadError::Http(status)),
            Err(error) => return Err(DownloadError::Failed(error.to_string())),
        };

        if !is_redirect(response.status()) {
            return Ok(response);
        }

        let location = response
            .header("location")
            .ok_or(DownloadError::RedirectWithoutLocation)?;
        url = resolve(&url, location);
    }

    Err(DownloadError::TooManyRedirects)
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

/// Resolves a `Location` against the URL it came from.
///
/// A relative target keeps the current origin, which is exactly why it still
/// has to be re-checked: the origin it keeps might already be one we accepted.
fn resolve(base: &str, location: &str) -> String {
    if location.contains("://") {
        return location.to_owned();
    }

    let origin_end = base
        .split_once("://")
        .map(|(scheme, rest)| {
            scheme.len() + 3 + rest.split(['/', '?', '#']).next().unwrap_or_default().len()
        })
        .unwrap_or(base.len());
    let origin = base.get(..origin_end).unwrap_or(base);

    if location.starts_with('/') {
        format!("{origin}{location}")
    } else {
        format!("{origin}/{location}")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn an_absolute_redirect_replaces_the_url() {
        assert_eq!(
            resolve(
                "https://github.com/x/y",
                "https://objects.githubusercontent.com/z"
            ),
            "https://objects.githubusercontent.com/z"
        );
    }

    #[test]
    fn a_rooted_redirect_keeps_the_origin() {
        assert_eq!(
            resolve("https://github.com/x/y", "/z/w"),
            "https://github.com/z/w"
        );
    }

    #[test]
    fn a_relative_redirect_keeps_the_origin() {
        assert_eq!(
            resolve("https://github.com/x/y", "z"),
            "https://github.com/z"
        );
    }

    /// The point of resolving by hand: whatever a redirect produces is a URL
    /// the allowlist still gets to refuse.
    #[test]
    fn a_redirect_off_github_is_still_refused_by_the_allowlist() {
        let target = resolve("https://github.com/x", "https://evil.example.com/payload");

        assert!(AllowedHost::check(&target).is_err());
    }

    /// Deliberately an assertion over constants: it is a guard on the cap
    /// itself, so that lowering `MAX_BYTES` below the largest artefact fails
    /// here rather than in a user's download.
    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn the_size_cap_is_bigger_than_the_biggest_artefact() {
        const LARGEST_ARTEFACT: u64 = 89 * 1024 * 1024;

        assert!(MAX_BYTES > LARGEST_ARTEFACT);
    }

    #[test]
    fn a_declared_size_over_the_cap_is_refused_before_connecting() {
        let agent = crate::net::agent();
        let cancel = AtomicBool::new(false);

        let result = fetch(
            &agent,
            "https://github.com/x",
            MAX_BYTES + 1,
            &cancel,
            &mut |_, _| {},
        );

        assert!(matches!(result, Err(DownloadError::TooLarge)));
    }

    #[test]
    fn a_forbidden_host_is_refused_before_connecting() {
        let agent = crate::net::agent();
        let cancel = AtomicBool::new(false);

        let result = fetch(
            &agent,
            "https://evil.example.com/x",
            10,
            &cancel,
            &mut |_, _| {},
        );

        assert!(matches!(result, Err(DownloadError::Forbidden(_))));
    }
}
