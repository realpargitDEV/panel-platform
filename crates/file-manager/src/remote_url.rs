//! Validating a URL the product is about to fetch from.
//!
//! A project may be created from a git remote or an archive URL, which means a
//! string the user typed decides what host this process connects to. The process
//! runs as the user: it can reach their loopback services, their LAN, and — on a
//! cloud host — the instance metadata endpoint. So a URL is not merely parsed
//! here, it is argued with.
//!
//! The split matters for testing. [`RemoteUrl::parse`] is pure syntax and needs
//! no network. [`address_is_forbidden`] is a pure predicate over an address.
//! Resolution is behind the [`HostResolver`] trait, so the interesting cases —
//! a hostname that resolves to `127.0.0.1`, a redirect chain ending in the
//! metadata address — are ordinary unit tests rather than something that needs a
//! DNS server.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};

use url::Url;

/// How many redirects a fetch may follow before giving up.
///
/// Every hop is re-validated, so this is not a safety control — it is a
/// termination control for a server that redirects in a loop.
pub const MAX_REDIRECTS: u8 = 5;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UrlError {
    #[error("the URL could not be parsed")]
    Malformed,
    /// Includes a redirect from `https` to `http`: a downgrade mid-fetch is how
    /// a plaintext hop appears in an otherwise encrypted transfer.
    #[error("only https:// URLs are accepted, not {scheme}://")]
    NotHttps { scheme: String },
    /// `https://user:token@host/...`. Refused rather than stripped, because a
    /// user who put a token in a URL needs to be told it was not used — silently
    /// dropping it would produce a confusing authentication failure instead.
    #[error("credentials must not be embedded in the URL; use the token field")]
    Userinfo,
    #[error("the URL has no host")]
    MissingHost,
    #[error("the host could not be resolved")]
    Unresolvable,
    /// The host resolved to an address inside this machine or its network.
    #[error("{host} resolves to {address}, which is {reason}")]
    ForbiddenAddress {
        host: String,
        address: IpAddr,
        reason: &'static str,
    },
    #[error("the server redirected more than {MAX_REDIRECTS} times")]
    TooManyRedirects,
}

/// A URL that has passed syntactic validation.
///
/// Passing one of these does not mean the host is allowed — that is
/// [`guard_host`], which needs to resolve it. The type exists so a function
/// cannot be handed a raw string by mistake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteUrl(Url);

impl RemoteUrl {
    pub fn parse(input: &str) -> Result<Self, UrlError> {
        let url = Url::parse(input.trim()).map_err(|_| UrlError::Malformed)?;

        if url.scheme() != "https" {
            return Err(UrlError::NotHttps {
                scheme: url.scheme().to_string(),
            });
        }

        if !url.username().is_empty() || url.password().is_some() {
            return Err(UrlError::Userinfo);
        }

        if url.host_str().is_none_or(str::is_empty) {
            return Err(UrlError::MissingHost);
        }

        Ok(Self(url))
    }

    pub fn host(&self) -> &str {
        // Guaranteed by `parse`; the fallback keeps this panic-free rather than
        // relying on that guarantee holding after a future edit.
        self.0.host_str().unwrap_or_default()
    }

    /// The port to connect to, defaulted for the scheme.
    pub fn port(&self) -> u16 {
        self.0.port().unwrap_or(443)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Resolve a `Location` header against this URL.
    ///
    /// Relative redirects are normal and must be supported; the result goes
    /// through [`RemoteUrl::parse`] again, so a relative redirect cannot smuggle
    /// in a scheme change and an absolute one cannot skip validation.
    pub fn redirect_to(&self, location: &str, hops_taken: u8) -> Result<Self, UrlError> {
        if hops_taken >= MAX_REDIRECTS {
            return Err(UrlError::TooManyRedirects);
        }
        let joined = self
            .0
            .join(location.trim())
            .map_err(|_| UrlError::Malformed)?;
        Self::parse(joined.as_str())
    }
}

impl fmt::Display for RemoteUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

/// Why an address must not be connected to, or `None` if it is fine.
///
/// Written out rather than using `IpAddr::is_global`, which is still unstable.
/// The list is deliberately generous: a false refusal costs a user one confused
/// support question, while a false acceptance is a request this process makes on
/// an attacker's behalf against a network only it can reach.
pub fn address_is_forbidden(address: IpAddr) -> Option<&'static str> {
    match address {
        IpAddr::V4(v4) => forbidden_v4(v4),
        // An IPv4-mapped address is an IPv4 address wearing a hat. Checking it
        // as v6 only would let `::ffff:127.0.0.1` through.
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => forbidden_v4(v4),
            None => forbidden_v6(v6),
        },
    }
}

fn forbidden_v4(address: Ipv4Addr) -> Option<&'static str> {
    let [a, b, ..] = address.octets();
    if address.is_loopback() {
        return Some("this machine");
    }
    if address.is_unspecified() {
        return Some("unspecified");
    }
    if address.is_private() {
        return Some("a private network");
    }
    if address.is_link_local() {
        // 169.254.169.254 lives here: the cloud instance metadata endpoint, and
        // the single most valuable target for a URL a user can be tricked into
        // pasting.
        return Some("link-local, which includes the cloud metadata endpoint");
    }
    if address.is_broadcast() {
        return Some("a broadcast address");
    }
    if address.is_multicast() {
        return Some("multicast");
    }
    if a == 100 && (64..128).contains(&b) {
        return Some("carrier-grade NAT space");
    }
    if a == 0 {
        return Some("reserved");
    }
    if a == 192 && b == 0 {
        return Some("reserved for protocol assignments");
    }
    if a == 198 && (b == 18 || b == 19) {
        return Some("benchmarking space");
    }
    if a >= 240 {
        return Some("reserved");
    }
    None
}

fn forbidden_v6(address: Ipv6Addr) -> Option<&'static str> {
    if address.is_loopback() {
        return Some("this machine");
    }
    if address.is_unspecified() {
        return Some("unspecified");
    }
    if address.is_multicast() {
        return Some("multicast");
    }
    let first = address.segments()[0];
    if first & 0xfe00 == 0xfc00 {
        return Some("a unique-local network");
    }
    if first & 0xffc0 == 0xfe80 {
        return Some("link-local");
    }
    None
}

/// Turns a hostname into addresses. Injected so the guard is testable.
pub trait HostResolver {
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<IpAddr>, UrlError>;
}

/// The real one.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemResolver;

impl HostResolver for SystemResolver {
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<IpAddr>, UrlError> {
        let addresses: Vec<IpAddr> = (host, port)
            .to_socket_addrs()
            .map_err(|_| UrlError::Unresolvable)?
            .map(|socket| socket.ip())
            .collect();
        if addresses.is_empty() {
            return Err(UrlError::Unresolvable);
        }
        Ok(addresses)
    }
}

/// Resolve a validated URL's host and refuse it if *any* address is forbidden.
///
/// Any, not all: a host answering with both a public and a loopback address is
/// how a DNS-rebinding attempt looks, and connecting would be a coin toss over
/// which address the socket picks.
pub fn guard_host<R: HostResolver>(url: &RemoteUrl, resolver: &R) -> Result<Vec<IpAddr>, UrlError> {
    let addresses = resolver.resolve(url.host(), url.port())?;
    for address in &addresses {
        if let Some(reason) = address_is_forbidden(*address) {
            return Err(UrlError::ForbiddenAddress {
                host: url.host().to_string(),
                address: *address,
                reason,
            });
        }
    }
    Ok(addresses)
}

/// Validate a string and its host in one step: what a caller about to open a
/// connection wants.
///
/// The addresses are returned rather than discarded because re-resolving at
/// connect time is a second answer to the same question, and the second answer
/// is the one a rebinding attack controls. A caller that connects to one of
/// these addresses cannot be redirected by DNS after the check.
pub fn validate<R: HostResolver>(
    input: &str,
    resolver: &R,
) -> Result<(RemoteUrl, Vec<IpAddr>), UrlError> {
    let url = RemoteUrl::parse(input)?;
    let addresses = guard_host(&url, resolver)?;
    Ok((url, addresses))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::str::FromStr;

    /// A resolver with a fixed table, so the hostile cases are deterministic.
    struct FakeResolver(HashMap<String, Vec<IpAddr>>);

    impl FakeResolver {
        fn with(host: &str, addresses: &[&str]) -> Self {
            let mut table = HashMap::new();
            table.insert(
                host.to_string(),
                addresses
                    .iter()
                    .map(|a| IpAddr::from_str(a).expect("test address"))
                    .collect(),
            );
            Self(table)
        }
    }

    impl HostResolver for FakeResolver {
        fn resolve(&self, host: &str, _port: u16) -> Result<Vec<IpAddr>, UrlError> {
            self.0.get(host).cloned().ok_or(UrlError::Unresolvable)
        }
    }

    fn public() -> FakeResolver {
        FakeResolver::with("github.com", &["140.82.121.4"])
    }

    // ------------------------------------------------------------ syntax

    #[test]
    fn an_ordinary_https_url_is_accepted() {
        let url = RemoteUrl::parse("https://github.com/owner/repo.git").expect("accepted");
        assert_eq!(url.host(), "github.com");
        assert_eq!(url.port(), 443);
    }

    #[test]
    fn surrounding_whitespace_is_tolerated() {
        // Pasted URLs arrive with it, and refusing would be pedantry.
        assert!(RemoteUrl::parse("  https://github.com/owner/repo.git\n").is_ok());
    }

    #[test]
    fn every_scheme_but_https_is_refused() {
        for input in [
            "http://github.com/owner/repo.git",
            "git://github.com/owner/repo.git",
            "ssh://git@github.com/owner/repo.git",
            "file:///C:/Windows/System32",
            "file:///etc/passwd",
            "data:text/plain,hello",
            "javascript:alert(1)",
            "ftp://example.com/repo.zip",
        ] {
            assert!(
                matches!(
                    RemoteUrl::parse(input),
                    Err(UrlError::NotHttps { .. } | UrlError::Malformed)
                ),
                "{input} should be refused"
            );
        }
    }

    #[test]
    fn a_token_in_the_url_is_refused_rather_than_stripped() {
        // Stripping would turn "I pasted my token in the wrong box" into an
        // authentication failure with no explanation.
        for input in [
            "https://user:ghp_token@github.com/owner/repo.git",
            "https://ghp_token@github.com/owner/repo.git",
        ] {
            assert_eq!(RemoteUrl::parse(input), Err(UrlError::Userinfo), "{input}");
        }
    }

    #[test]
    fn a_url_with_no_host_is_refused() {
        for input in ["https://", "https://:443/repo.git"] {
            assert!(
                matches!(
                    RemoteUrl::parse(input),
                    Err(UrlError::MissingHost | UrlError::Malformed)
                ),
                "{input} should be refused"
            );
        }
    }

    #[test]
    fn an_extra_slash_does_not_invent_a_host() {
        // Worth pinning: for special schemes the URL standard *skips* surplus
        // authority slashes, so `https:///owner/repo.git` is not host-less — the
        // host is `owner`. A reader assuming otherwise would think the
        // `MissingHost` arm covers this, and it does not.
        let url = RemoteUrl::parse("https:///owner/repo.git").expect("has a host");
        assert_eq!(url.host(), "owner");
    }

    #[test]
    fn nonsense_is_refused() {
        for input in ["", "   ", "not a url", "https//github.com", "://"] {
            assert!(
                RemoteUrl::parse(input).is_err(),
                "{input:?} should be refused"
            );
        }
    }

    #[test]
    fn an_explicit_port_is_kept() {
        let url = RemoteUrl::parse("https://git.example.com:8443/repo.git").expect("accepted");
        assert_eq!(url.port(), 8443);
    }

    // ----------------------------------------------------------- addresses

    #[test]
    fn addresses_inside_this_machine_or_its_network_are_forbidden() {
        for address in [
            "127.0.0.1",
            "127.1.2.3",
            "0.0.0.0",
            "10.0.0.5",
            "172.16.4.9",
            "172.31.255.254",
            "192.168.1.10",
            "169.254.169.254", // cloud instance metadata
            "169.254.1.1",
            "100.64.0.1",
            "255.255.255.255",
            "224.0.0.1",
            "240.0.0.1",
            "198.18.0.1",
            "::1",
            "::",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "ff02::1",
            "::ffff:127.0.0.1", // IPv4-mapped loopback
            "::ffff:10.0.0.1",
        ] {
            let parsed = IpAddr::from_str(address).expect("test address");
            assert!(
                address_is_forbidden(parsed).is_some(),
                "{address} should be forbidden"
            );
        }
    }

    #[test]
    fn ordinary_public_addresses_are_allowed() {
        for address in [
            "140.82.121.4", // github.com
            "1.1.1.1",
            "8.8.8.8",
            "172.32.0.1", // just outside 172.16/12
            "192.169.0.1",
            "2606:4700::1111",
        ] {
            let parsed = IpAddr::from_str(address).expect("test address");
            assert_eq!(
                address_is_forbidden(parsed),
                None,
                "{address} should be allowed"
            );
        }
    }

    #[test]
    fn the_metadata_endpoint_is_named_in_its_refusal() {
        // The message is the point: a user seeing this should understand what
        // their URL was about to do.
        let reason =
            address_is_forbidden(IpAddr::from_str("169.254.169.254").unwrap()).expect("forbidden");
        assert!(reason.contains("metadata"), "unhelpful reason: {reason}");
    }

    // --------------------------------------------------------------- guard

    #[test]
    fn a_host_resolving_to_a_public_address_passes() {
        let url = RemoteUrl::parse("https://github.com/owner/repo.git").unwrap();
        let addresses = guard_host(&url, &public()).expect("allowed");
        assert_eq!(addresses.len(), 1);
    }

    #[test]
    fn a_hostname_pointing_at_loopback_is_refused() {
        // The interesting case: nothing about "totally-legit.example.com" looks
        // wrong until it is resolved.
        let resolver = FakeResolver::with("totally-legit.example.com", &["127.0.0.1"]);
        let url = RemoteUrl::parse("https://totally-legit.example.com/repo.git").unwrap();
        assert!(matches!(
            guard_host(&url, &resolver),
            Err(UrlError::ForbiddenAddress { .. })
        ));
    }

    #[test]
    fn one_bad_address_among_good_ones_refuses_the_host() {
        // What rebinding looks like. Connecting would be a coin toss.
        let resolver = FakeResolver::with("split.example.com", &["140.82.121.4", "127.0.0.1"]);
        let url = RemoteUrl::parse("https://split.example.com/repo.git").unwrap();
        assert!(matches!(
            guard_host(&url, &resolver),
            Err(UrlError::ForbiddenAddress { .. })
        ));
    }

    #[test]
    fn a_host_that_does_not_resolve_is_refused() {
        let url = RemoteUrl::parse("https://nowhere.example.com/repo.git").unwrap();
        assert_eq!(guard_host(&url, &public()), Err(UrlError::Unresolvable));
    }

    #[test]
    fn validate_returns_the_addresses_it_checked() {
        // A caller that connects to these cannot be re-pointed by a second DNS
        // answer.
        let (url, addresses) =
            validate("https://github.com/owner/repo.git", &public()).expect("valid");
        assert_eq!(url.host(), "github.com");
        assert_eq!(addresses, vec![IpAddr::from_str("140.82.121.4").unwrap()]);
    }

    // ----------------------------------------------------------- redirects

    #[test]
    fn a_relative_redirect_is_resolved_against_the_current_url() {
        let url = RemoteUrl::parse("https://example.com/a/b").unwrap();
        let next = url.redirect_to("../c/release.zip", 0).expect("redirect");
        assert_eq!(next.as_str(), "https://example.com/c/release.zip");
    }

    #[test]
    fn a_redirect_to_http_is_refused() {
        let url = RemoteUrl::parse("https://example.com/a").unwrap();
        assert!(matches!(
            url.redirect_to("http://example.com/a", 0),
            Err(UrlError::NotHttps { .. })
        ));
    }

    #[test]
    fn a_redirect_carrying_userinfo_is_refused() {
        let url = RemoteUrl::parse("https://example.com/a").unwrap();
        assert_eq!(
            url.redirect_to("https://user:token@example.com/a", 0),
            Err(UrlError::Userinfo)
        );
    }

    #[test]
    fn a_redirect_chain_ending_at_a_private_address_is_refused() {
        // The reason every hop is re-resolved rather than only the first.
        let resolver = FakeResolver::with("internal.example.com", &["10.1.2.3"]);
        let first = RemoteUrl::parse("https://downloads.example.com/latest").unwrap();
        let second = first
            .redirect_to("https://internal.example.com/secret.zip", 0)
            .expect("syntactically fine");
        assert!(matches!(
            guard_host(&second, &resolver),
            Err(UrlError::ForbiddenAddress { .. })
        ));
    }

    #[test]
    fn redirects_stop_at_the_limit() {
        let url = RemoteUrl::parse("https://example.com/a").unwrap();
        assert!(url
            .redirect_to("https://example.com/b", MAX_REDIRECTS - 1)
            .is_ok());
        assert_eq!(
            url.redirect_to("https://example.com/b", MAX_REDIRECTS),
            Err(UrlError::TooManyRedirects)
        );
    }
}
