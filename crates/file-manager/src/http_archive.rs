//! Fetching a project from an HTTPS archive URL.
//!
//! The transport is behind a trait for the same reason the entry rules live
//! apart from the ZIP reader: the interesting behaviour here is not "can it make
//! an HTTPS request" but what it does with a redirect, an oversized body, or a
//! server that would like the caller's token. Those are tested against a fake
//! transport, with no network and no loopback listener — which matters, because a
//! test server on `127.0.0.1` is an address [`crate::remote_url`] exists to
//! refuse.
//!
//! [`ReqwestTransport`] is therefore kept as small as it can be: one GET, no
//! redirect following, no cookie jar, no retry.

use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use crate::remote_url::{guard_host, HostResolver, RemoteUrl, UrlError};
use crate::zip_import::{check_ratio, ArchiveError, ArchiveLimits, ImportReport, Staging};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    #[error(transparent)]
    Url(#[from] UrlError),
    /// Connection-level failure. The message is the transport's, which may name
    /// the host but never a credential — the token is a header, not part of the
    /// URL, precisely so that it cannot appear here.
    #[error("the download failed: {0}")]
    Transport(String),
    #[error("the server answered {code}")]
    Status { code: u16 },
    #[error("the server redirected without saying where")]
    RedirectWithoutLocation,
    #[error("the download exceeded the {limit} byte limit")]
    TooLarge { limit: u64 },
    #[error("the server returned an empty body")]
    Empty,
    #[error("the download is neither a ZIP nor a gzipped tar archive")]
    UnknownFormat,
    #[error(transparent)]
    Archive(#[from] ArchiveError),
    #[error("writing the download failed: {0}")]
    Io(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FetchLimits {
    /// Ceiling on the downloaded bytes, enforced while streaming rather than
    /// from `Content-Length`, which a server is free to lie about.
    pub max_bytes: u64,
    pub timeout: Duration,
}

impl Default for FetchLimits {
    fn default() -> Self {
        Self {
            max_bytes: 2 * 1024 * 1024 * 1024,
            timeout: Duration::from_secs(300),
        }
    }
}

/// One HTTP response, with the body left unread.
pub struct HttpResponse {
    pub status: u16,
    pub location: Option<String>,
    pub body: Box<dyn Read>,
}

impl std::fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpResponse")
            .field("status", &self.status)
            .field("location", &self.location)
            .finish_non_exhaustive()
    }
}

/// A single GET that does **not** follow redirects.
///
/// Following them is this module's job, because each hop has to be validated and
/// re-resolved before it is followed, and a client library that follows them
/// internally would do neither.
pub trait HttpTransport {
    fn get(
        &self,
        url: &RemoteUrl,
        token: Option<&str>,
        timeout: Duration,
    ) -> Result<HttpResponse, FetchError>;
}

/// The real transport.
#[derive(Debug, Clone, Default)]
pub struct ReqwestTransport;

impl HttpTransport for ReqwestTransport {
    fn get(
        &self,
        url: &RemoteUrl,
        token: Option<&str>,
        timeout: Duration,
    ) -> Result<HttpResponse, FetchError> {
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout)
            .https_only(true)
            .build()
            .map_err(|error| FetchError::Transport(error.to_string()))?;

        let mut request = client.get(url.as_str());
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .map_err(|error| FetchError::Transport(error.to_string()))?;

        let status = response.status().as_u16();
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);

        Ok(HttpResponse {
            status,
            location,
            body: Box::new(response),
        })
    }
}

/// Download a URL to a file, validating every hop.
///
/// Returns the number of bytes written.
///
/// The token is sent only to the host the user named. A server that redirects
/// elsewhere gets the request without it: forwarding an `Authorization` header
/// across a redirect hands the user's credential to whoever the first server
/// chooses to point at.
pub fn download<T: HttpTransport, R: HostResolver>(
    input: &str,
    token: Option<&str>,
    transport: &T,
    resolver: &R,
    destination: &Path,
    limits: &FetchLimits,
) -> Result<u64, FetchError> {
    let mut url = RemoteUrl::parse(input)?;
    guard_host(&url, resolver)?;
    let original_host = url.host().to_string();

    let mut hops = 0u8;
    loop {
        let token_for_this_hop = if url.host() == original_host {
            token
        } else {
            None
        };

        let response = transport.get(&url, token_for_this_hop, limits.timeout)?;

        match response.status {
            200 => return stream_to_file(response.body, destination, limits.max_bytes),
            301 | 302 | 303 | 307 | 308 => {
                let location = response
                    .location
                    .ok_or(FetchError::RedirectWithoutLocation)?;
                url = url.redirect_to(&location, hops)?;
                // Re-resolved, not merely re-parsed: the redirect target is a
                // new host and gets the same scrutiny as the first one.
                guard_host(&url, resolver)?;
                hops = hops.saturating_add(1);
            }
            code => return Err(FetchError::Status { code }),
        }
    }
}

/// Copy a body to a file, stopping at the limit.
fn stream_to_file(
    mut body: Box<dyn Read>,
    destination: &Path,
    max_bytes: u64,
) -> Result<u64, FetchError> {
    let file = std::fs::File::create(destination).map_err(|e| FetchError::Io(e.to_string()))?;
    let mut writer = std::io::BufWriter::new(file);

    let mut written = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = body
            .read(&mut buffer)
            .map_err(|error| FetchError::Transport(error.to_string()))?;
        if read == 0 {
            break;
        }
        written = written.saturating_add(read as u64);
        if written > max_bytes {
            // The partial file is removed rather than left for a caller to
            // remember: this runs inside a staging directory, but the whole
            // point of the limit is not filling the disk.
            drop(writer);
            let _ = std::fs::remove_file(destination);
            return Err(FetchError::TooLarge { limit: max_bytes });
        }
        writer
            .write_all(buffer.get(..read).unwrap_or_default())
            .map_err(|error| FetchError::Io(error.to_string()))?;
    }

    writer
        .flush()
        .map_err(|error| FetchError::Io(error.to_string()))?;

    if written == 0 {
        let _ = std::fs::remove_file(destination);
        return Err(FetchError::Empty);
    }
    Ok(written)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    TarGzip,
}

/// Identify an archive by its first bytes rather than by its URL.
///
/// A URL's extension is a claim made by whoever wrote the URL. The magic number
/// is a claim made by the bytes that will actually be extracted, which is the
/// one worth acting on.
pub fn sniff(prefix: &[u8]) -> Option<ArchiveFormat> {
    match prefix {
        [0x50, 0x4b, 0x03, 0x04, ..] | [0x50, 0x4b, 0x05, 0x06, ..] => Some(ArchiveFormat::Zip),
        [0x1f, 0x8b, ..] => Some(ArchiveFormat::TarGzip),
        _ => None,
    }
}

fn sniff_file(path: &Path) -> Result<ArchiveFormat, FetchError> {
    let mut file = std::fs::File::open(path).map_err(|e| FetchError::Io(e.to_string()))?;
    let mut prefix = [0u8; 4];
    let read = file
        .read(&mut prefix)
        .map_err(|e| FetchError::Io(e.to_string()))?;
    sniff(prefix.get(..read).unwrap_or_default()).ok_or(FetchError::UnknownFormat)
}

/// Everything an archive import needs, grouped so the call site reads as a
/// description of the import rather than nine positional arguments.
#[derive(Debug, Clone, Copy)]
pub struct RemoteArchiveRequest<'a> {
    pub url: &'a str,
    pub token: Option<&'a str>,
    pub staging_root: &'a Path,
    pub destination: &'a Path,
    /// Names the staging directory. Generated by the application, never taken
    /// from a caller, so a hostile URL cannot influence where the download lands.
    pub import_id: &'a str,
    pub fetch_limits: FetchLimits,
    pub archive_limits: ArchiveLimits,
}

/// Fetch an archive URL into a new project directory.
///
/// The download lands in a UUID-named staging directory, is identified by its
/// bytes, and is extracted through exactly the same entry rules as an uploaded
/// ZIP. On any failure the staging directory removes itself and no project
/// exists — a hostile archive from a URL is not a different threat from a
/// hostile archive from an upload, and is not treated as one.
pub fn import_remote_archive<T: HttpTransport, R: HostResolver>(
    request: &RemoteArchiveRequest<'_>,
    transport: &T,
    resolver: &R,
) -> Result<ImportReport, FetchError> {
    let RemoteArchiveRequest {
        url: input,
        token,
        staging_root,
        destination,
        import_id,
        fetch_limits,
        archive_limits,
    } = *request;

    if destination.exists() {
        return Err(FetchError::Io("the destination already exists".to_string()));
    }

    let staging = Staging::new(staging_root, import_id)?;

    // The archive is downloaded *beside* the tree it expands into, both inside
    // the staging directory, so a failure at any point leaves neither.
    let download_path = staging.path().join("download.bin");
    let tree = staging.path().join("tree");
    std::fs::create_dir_all(&tree).map_err(|e| FetchError::Io(e.to_string()))?;

    let downloaded = download(
        input,
        token,
        transport,
        resolver,
        &download_path,
        &fetch_limits,
    )?;

    let report = match sniff_file(&download_path)? {
        ArchiveFormat::Zip => {
            let file =
                std::fs::File::open(&download_path).map_err(|e| FetchError::Io(e.to_string()))?;
            crate::extract::extract_into(std::io::BufReader::new(file), &tree, &archive_limits)?
        }
        ArchiveFormat::TarGzip => {
            let file =
                std::fs::File::open(&download_path).map_err(|e| FetchError::Io(e.to_string()))?;
            crate::extract::extract_tar_gzip_into(
                std::io::BufReader::new(file),
                &tree,
                &archive_limits,
                downloaded,
            )?
        }
    };

    // Removed before the promote so it does not become a file inside the user's
    // project.
    std::fs::remove_file(&download_path).map_err(|e| FetchError::Io(e.to_string()))?;

    promote_tree(staging, &tree, destination)?;
    Ok(report)
}

/// Move the extracted tree into place, then let the staging directory go.
fn promote_tree(staging: Staging, tree: &Path, destination: &Path) -> Result<(), FetchError> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|e| FetchError::Io(e.to_string()))?;
    }
    std::fs::rename(tree, destination).map_err(|e| FetchError::Io(e.to_string()))?;
    // `staging` is dropped here and removes what is left of itself, which is now
    // an empty directory. The tree is already out.
    drop(staging);
    Ok(())
}

/// The overall expansion check, exposed for the tar path where per-entry
/// compressed sizes do not exist: the downloaded byte count is the compressed
/// total, which is a truer ratio than summing per-entry claims.
pub fn check_download_ratio(
    downloaded: u64,
    expanded: u64,
    limits: &ArchiveLimits,
) -> Result<(), ArchiveError> {
    check_ratio(downloaded, expanded, limits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_url::{UrlError, MAX_REDIRECTS};
    use std::cell::RefCell;
    use std::io::Cursor;
    use std::net::IpAddr;
    use std::str::FromStr;

    /// Resolves everything to one public address, so these tests exercise the
    /// redirect and body logic rather than the address guard.
    struct AnyPublic;

    impl HostResolver for AnyPublic {
        fn resolve(&self, _host: &str, _port: u16) -> Result<Vec<IpAddr>, UrlError> {
            Ok(vec![IpAddr::from_str("140.82.121.4").expect("address")])
        }
    }

    /// Resolves one named host to a private address and everything else to a
    /// public one: a redirect into an internal network.
    struct PrivateFor(&'static str);

    impl HostResolver for PrivateFor {
        fn resolve(&self, host: &str, _port: u16) -> Result<Vec<IpAddr>, UrlError> {
            let address = if host == self.0 {
                "10.1.2.3"
            } else {
                "140.82.121.4"
            };
            Ok(vec![IpAddr::from_str(address).expect("address")])
        }
    }

    /// What a scripted transport should do for one request.
    enum Step {
        Body(Vec<u8>),
        Redirect(&'static str),
        Status(u16),
        NoLocationRedirect,
    }

    /// Replays scripted steps and records what it was asked for, so a test can
    /// assert on the request as well as the outcome.
    struct FakeTransport {
        steps: RefCell<Vec<Step>>,
        seen: RefCell<Vec<(String, Option<String>)>>,
    }

    impl FakeTransport {
        fn new(steps: Vec<Step>) -> Self {
            // Reversed so `pop` walks them in order.
            let mut steps = steps;
            steps.reverse();
            Self {
                steps: RefCell::new(steps),
                seen: RefCell::new(Vec::new()),
            }
        }

        fn requests(&self) -> Vec<(String, Option<String>)> {
            self.seen.borrow().clone()
        }
    }

    impl HttpTransport for FakeTransport {
        fn get(
            &self,
            url: &RemoteUrl,
            token: Option<&str>,
            _timeout: Duration,
        ) -> Result<HttpResponse, FetchError> {
            self.seen
                .borrow_mut()
                .push((url.as_str().to_string(), token.map(str::to_string)));

            match self.steps.borrow_mut().pop() {
                Some(Step::Body(bytes)) => Ok(HttpResponse {
                    status: 200,
                    location: None,
                    body: Box::new(Cursor::new(bytes)),
                }),
                Some(Step::Redirect(to)) => Ok(HttpResponse {
                    status: 302,
                    location: Some(to.to_string()),
                    body: Box::new(Cursor::new(Vec::new())),
                }),
                Some(Step::NoLocationRedirect) => Ok(HttpResponse {
                    status: 302,
                    location: None,
                    body: Box::new(Cursor::new(Vec::new())),
                }),
                Some(Step::Status(code)) => Ok(HttpResponse {
                    status: code,
                    location: None,
                    body: Box::new(Cursor::new(Vec::new())),
                }),
                None => Err(FetchError::Transport("no more scripted steps".to_string())),
            }
        }
    }

    fn download_with(
        steps: Vec<Step>,
        token: Option<&str>,
    ) -> (
        tempfile::TempDir,
        FakeTransport,
        Result<u64, FetchError>,
        std::path::PathBuf,
    ) {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("out.bin");
        let transport = FakeTransport::new(steps);
        let result = download(
            "https://downloads.example.com/latest.zip",
            token,
            &transport,
            &AnyPublic,
            &target,
            &FetchLimits::default(),
        );
        (dir, transport, result, target)
    }

    #[test]
    fn a_plain_download_is_written_to_disk() {
        let (_dir, _transport, result, target) =
            download_with(vec![Step::Body(b"archive bytes".to_vec())], None);
        assert_eq!(result.expect("downloaded"), 13);
        assert_eq!(std::fs::read(&target).expect("read"), b"archive bytes");
    }

    #[test]
    fn a_redirect_is_followed_after_being_revalidated() {
        let (_dir, transport, result, _target) = download_with(
            vec![
                Step::Redirect("https://cdn.example.com/real.zip"),
                Step::Body(b"ok".to_vec()),
            ],
            None,
        );
        result.expect("downloaded");
        let requests = transport.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].0, "https://cdn.example.com/real.zip");
    }

    #[test]
    fn a_token_is_not_forwarded_to_a_host_the_user_did_not_name() {
        // The leak this prevents: any server that can answer the first request
        // could otherwise redirect to itself-elsewhere and collect the token.
        let (_dir, transport, result, _target) = download_with(
            vec![
                Step::Redirect("https://someone-elses-host.example.net/real.zip"),
                Step::Body(b"ok".to_vec()),
            ],
            Some("ghp_secret"),
        );
        result.expect("downloaded");

        let requests = transport.requests();
        assert_eq!(requests[0].1.as_deref(), Some("ghp_secret"));
        assert_eq!(
            requests[1].1, None,
            "the token was forwarded to {}",
            requests[1].0
        );
    }

    #[test]
    fn a_token_survives_a_redirect_within_the_same_host() {
        // Otherwise a private repository whose host redirects `/latest` to
        // `/v1.2.3` would fail to authenticate for no visible reason.
        let (_dir, transport, result, _target) = download_with(
            vec![
                Step::Redirect("https://downloads.example.com/v1.2.3.zip"),
                Step::Body(b"ok".to_vec()),
            ],
            Some("ghp_secret"),
        );
        result.expect("downloaded");
        assert_eq!(transport.requests()[1].1.as_deref(), Some("ghp_secret"));
    }

    #[test]
    fn a_redirect_into_a_private_network_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let transport = FakeTransport::new(vec![
            Step::Redirect("https://internal.example.com/secrets.zip"),
            Step::Body(b"should never be read".to_vec()),
        ]);

        let result = download(
            "https://downloads.example.com/latest.zip",
            None,
            &transport,
            &PrivateFor("internal.example.com"),
            &dir.path().join("out.bin"),
            &FetchLimits::default(),
        );

        assert!(
            matches!(
                result,
                Err(FetchError::Url(UrlError::ForbiddenAddress { .. }))
            ),
            "got {result:?}"
        );
        assert_eq!(
            transport.requests().len(),
            1,
            "the second request must never be made"
        );
    }

    #[test]
    fn a_redirect_to_http_is_refused_mid_download() {
        let (_dir, _transport, result, _target) = download_with(
            vec![Step::Redirect("http://downloads.example.com/x.zip")],
            None,
        );
        assert!(matches!(
            result,
            Err(FetchError::Url(UrlError::NotHttps { .. }))
        ));
    }

    #[test]
    fn an_endless_redirect_loop_terminates() {
        let steps = (0..MAX_REDIRECTS + 2)
            .map(|_| Step::Redirect("https://downloads.example.com/again"))
            .collect();
        let (_dir, _transport, result, _target) = download_with(steps, None);
        assert!(matches!(
            result,
            Err(FetchError::Url(UrlError::TooManyRedirects))
        ));
    }

    #[test]
    fn a_redirect_without_a_location_is_an_error_not_a_hang() {
        let (_dir, _transport, result, _target) =
            download_with(vec![Step::NoLocationRedirect], None);
        assert_eq!(result, Err(FetchError::RedirectWithoutLocation));
    }

    #[test]
    fn an_error_status_is_reported_with_its_code() {
        for code in [401u16, 403, 404, 500] {
            let (_dir, _transport, result, _target) = download_with(vec![Step::Status(code)], None);
            assert_eq!(result, Err(FetchError::Status { code }));
        }
    }

    #[test]
    fn a_body_over_the_limit_is_stopped_and_the_partial_file_removed() {
        // The cap is on bytes actually received, so a lying Content-Length
        // changes nothing.
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("out.bin");
        let transport = FakeTransport::new(vec![Step::Body(vec![0u8; 5000])]);

        let result = download(
            "https://downloads.example.com/big.zip",
            None,
            &transport,
            &AnyPublic,
            &target,
            &FetchLimits {
                max_bytes: 1000,
                ..FetchLimits::default()
            },
        );

        assert_eq!(result, Err(FetchError::TooLarge { limit: 1000 }));
        assert!(!target.exists(), "a partial download was left on disk");
    }

    #[test]
    fn an_empty_body_is_an_error() {
        let (_dir, _transport, result, target) = download_with(vec![Step::Body(Vec::new())], None);
        assert_eq!(result, Err(FetchError::Empty));
        assert!(!target.exists());
    }

    #[test]
    fn a_url_that_never_passes_validation_makes_no_request() {
        let dir = tempfile::tempdir().expect("temp dir");
        let transport = FakeTransport::new(vec![Step::Body(b"x".to_vec())]);
        let result = download(
            "http://downloads.example.com/latest.zip",
            None,
            &transport,
            &AnyPublic,
            &dir.path().join("out.bin"),
            &FetchLimits::default(),
        );
        assert!(matches!(
            result,
            Err(FetchError::Url(UrlError::NotHttps { .. }))
        ));
        assert!(transport.requests().is_empty());
    }

    // ------------------------------------------------------------- sniffing

    #[test]
    fn archives_are_identified_by_their_bytes() {
        assert_eq!(sniff(b"PK\x03\x04rest"), Some(ArchiveFormat::Zip));
        assert_eq!(sniff(b"PK\x05\x06"), Some(ArchiveFormat::Zip));
        assert_eq!(
            sniff(&[0x1f, 0x8b, 0x08, 0x00]),
            Some(ArchiveFormat::TarGzip)
        );
        assert_eq!(sniff(b"not an archive"), None);
        assert_eq!(sniff(b""), None);
    }

    #[test]
    fn a_url_claiming_zip_while_serving_something_else_is_refused() {
        // The extension is the URL author's claim; the magic number is the
        // bytes' own.
        let dir = tempfile::tempdir().expect("temp dir");
        let transport = FakeTransport::new(vec![Step::Body(b"<html>nope</html>".to_vec())]);
        let destination = dir.path().join("projects/prj_1");
        let result = import_remote_archive(
            &RemoteArchiveRequest {
                url: "https://downloads.example.com/looks-like.zip",
                token: None,
                staging_root: dir.path(),
                destination: &destination,
                import_id: "sniff-test",
                fetch_limits: FetchLimits::default(),
                archive_limits: ArchiveLimits::default(),
            },
            &transport,
            &AnyPublic,
        );
        assert_eq!(result, Err(FetchError::UnknownFormat));
        assert!(!dir.path().join("projects/prj_1").exists());
        assert!(!dir.path().join("import-sniff-test").exists());
    }
}
