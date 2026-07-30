//! Where this program is allowed to talk to.
//!
//! The rules are `crates/updater`'s, for the same reason: a program that
//! downloads and then executes code must not be steerable to an arbitrary host
//! by anything it downloaded.

pub const USER_AGENT: &str = concat!("panel-platform-setup/", env!("CARGO_PKG_VERSION"));

/// Redirects are followed, so each hop is checked against this list too — an
/// allowlist that only covers the first request is decoration.
const ALLOWED: &[&str] = &[
    "api.github.com",
    "github.com",
    // Where release assets actually live once GitHub redirects.
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum NetError {
    #[error("{0} is not an https URL")]
    NotHttps(String),
    #[error("{0} is not a host this installer will download from")]
    ForbiddenHost(String),
    #[error("{0} is not a URL")]
    Malformed(String),
}

#[derive(Debug)]
pub struct AllowedHost;

impl AllowedHost {
    /// Accepts a URL only if it is `https` and its host is on the list.
    ///
    /// The scheme is compared exactly: `HTTPS://` is fine, `http` is not, and
    /// neither is anything that merely begins with the right letters.
    pub fn check(url: &str) -> Result<(), NetError> {
        let Some((scheme, rest)) = url.split_once("://") else {
            return Err(NetError::Malformed(url.to_owned()));
        };

        if !scheme.eq_ignore_ascii_case("https") {
            return Err(NetError::NotHttps(url.to_owned()));
        }

        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();

        // Userinfo would let `https://api.github.com@evil.example.com/` read as
        // trusted to anyone skimming, so a URL carrying any is refused outright
        // rather than parsed around.
        if authority.contains('@') {
            return Err(NetError::ForbiddenHost(url.to_owned()));
        }

        let host = authority
            .rsplit_once(':')
            .map_or(authority, |(host, _port)| host);

        if host.is_empty() {
            return Err(NetError::Malformed(url.to_owned()));
        }

        if ALLOWED
            .iter()
            .any(|allowed| host.eq_ignore_ascii_case(allowed))
        {
            Ok(())
        } else {
            Err(NetError::ForbiddenHost(url.to_owned()))
        }
    }
}

/// One agent for the whole run, with timeouts set so a stalled connection fails
/// instead of showing a progress bar forever.
pub fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(20))
        .timeout_read(std::time::Duration::from_secs(60))
        .user_agent(USER_AGENT)
        // Redirects are followed by hand in `download`, so each hop can be
        // checked before it is taken.
        .redirects(0)
        .build()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn github_over_https_is_allowed() {
        for url in [
            "https://api.github.com/repos/x/y/releases/latest",
            "https://github.com/x/y/releases/download/v1/a.deb",
            "https://objects.githubusercontent.com/blob",
            "https://release-assets.githubusercontent.com/blob",
            "HTTPS://github.com/x",
        ] {
            assert_eq!(AllowedHost::check(url), Ok(()), "{url}");
        }
    }

    #[test]
    fn plain_http_is_refused_even_for_an_allowed_host() {
        assert_eq!(
            AllowedHost::check("http://github.com/x"),
            Err(NetError::NotHttps("http://github.com/x".to_owned()))
        );
    }

    #[test]
    fn another_host_is_refused() {
        assert!(matches!(
            AllowedHost::check("https://evil.example.com/x"),
            Err(NetError::ForbiddenHost(_))
        ));
    }

    /// `https://github.com.evil.example/` and friends must not pass by prefix.
    #[test]
    fn a_lookalike_host_is_refused() {
        for url in [
            "https://github.com.evil.example/x",
            "https://notgithub.com/x",
            "https://evil.example/github.com",
            "https://api.github.com.evil.example/x",
        ] {
            assert!(
                matches!(AllowedHost::check(url), Err(NetError::ForbiddenHost(_))),
                "{url} was allowed"
            );
        }
    }

    /// The classic disguise: everything before the `@` is userinfo, and the
    /// real host is the part a reader skims past.
    #[test]
    fn userinfo_cannot_disguise_the_host() {
        assert!(matches!(
            AllowedHost::check("https://api.github.com@evil.example.com/x"),
            Err(NetError::ForbiddenHost(_))
        ));
    }

    #[test]
    fn a_port_does_not_change_the_host() {
        assert_eq!(AllowedHost::check("https://github.com:443/x"), Ok(()));
    }

    #[test]
    fn nonsense_is_refused() {
        for url in ["", "github.com/x", "https://", "https:///path"] {
            assert!(AllowedHost::check(url).is_err(), "{url} was allowed");
        }
    }
}
