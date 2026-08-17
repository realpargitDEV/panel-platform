//! Finding out what the latest published release is.
//!
//! Parsing is separate from fetching so that the awkward responses — a 404 for
//! a repository that has only drafts, a rate limit, a release with no assets —
//! are tested against recorded JSON rather than against GitHub.

use serde::Deserialize;

use crate::net::{self, AllowedHost};

/// The repository this build installs from.
///
/// GitHub answers a renamed owner with `301 Moved Permanently` rather than the
/// release, and this agent does not follow redirects — each hop is checked by
/// hand in `download` — so a stale name here is not a redirect that quietly
/// works. It is a setup program that cannot find anything to install, which is
/// what happened when `realpargitDEV` became `paar-git`.
const LATEST: &str = "https://api.github.com/repos/paar-git/panel-platform/releases/latest";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// The tag with any leading `v` removed, for display.
    pub version: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ReleaseError {
    /// Not a failure. `releases/latest` excludes drafts, which is correct: a
    /// draft is by definition not fit to install, and this program must not be
    /// able to reach one. Reported in the words a reader can act on rather than
    /// as a network error for a server that answered perfectly well.
    #[error("Panel Platform has no published release yet. Nothing is available to install.")]
    NoPublishedRelease,
    #[error("GitHub is rate limiting this connection. Try again {0}.")]
    RateLimited(String),
    #[error("GitHub answered {0} when asked for the latest release")]
    Http(u16),
    #[error("could not reach GitHub: {0}")]
    Unreachable(String),
    /// A redirect, which this agent does not follow. It means the address
    /// compiled into this program is not where the repository lives any more,
    /// and every copy already handed out has the same stale address. Said
    /// plainly, because the alternative is what shipped: the redirect's body
    /// has no `tag_name`, so it surfaced as "could not understand GitHub's
    /// answer" — which reads like GitHub changed its API rather than like this
    /// program is looking in the wrong place.
    #[error(
        "GitHub redirected the request for the latest release ({0}), which means this \
         build is looking for a repository that has moved."
    )]
    Moved(u16),
    #[error("could not understand GitHub's answer: {0}")]
    Malformed(String),
}

/// The subset of the API response this program uses. Everything else in it is
/// ignored rather than modelled.
#[derive(Debug, Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Debug, Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

pub fn fetch_latest(agent: &ureq::Agent) -> Result<Release, ReleaseError> {
    let response = agent
        .get(LATEST)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", net::USER_AGENT)
        .call();

    let body = match response {
        // `redirects(0)` hands a 3xx back as a response rather than following
        // it, so this has to be looked at before the body is parsed — a
        // redirect body is valid JSON that simply is not a release.
        Ok(response) if (300..400).contains(&response.status()) => {
            return Err(ReleaseError::Moved(response.status()));
        }
        Ok(response) => response
            .into_string()
            .map_err(|error| ReleaseError::Unreachable(error.to_string()))?,
        Err(ureq::Error::Status(404, _)) => return Err(ReleaseError::NoPublishedRelease),
        Err(ureq::Error::Status(403, response)) | Err(ureq::Error::Status(429, response)) => {
            // A 403 from this endpoint is nearly always the anonymous rate
            // limit, and saying "forbidden" would send a reader looking for a
            // permission problem they do not have.
            return Err(if response.header("x-ratelimit-remaining") == Some("0") {
                ReleaseError::RateLimited(reset_hint(response.header("x-ratelimit-reset")))
            } else {
                ReleaseError::Http(403)
            });
        }
        Err(ureq::Error::Status(status, _)) => return Err(ReleaseError::Http(status)),
        Err(error) => return Err(ReleaseError::Unreachable(error.to_string())),
    };

    parse(&body)
}

fn reset_hint(header: Option<&str>) -> String {
    let Some(reset) = header.and_then(|value| value.parse::<u64>().ok()) else {
        return "in a little while".to_owned();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0);

    match reset.saturating_sub(now) {
        0 => "now".to_owned(),
        seconds if seconds < 120 => format!("in {seconds} seconds"),
        seconds => format!("in {} minutes", seconds / 60),
    }
}

pub fn parse(body: &str) -> Result<Release, ReleaseError> {
    let api: ApiRelease =
        serde_json::from_str(body).map_err(|error| ReleaseError::Malformed(error.to_string()))?;

    // The endpoint is documented to exclude drafts. If one ever arrives anyway,
    // it is refused here rather than trusted because of where it came from.
    if api.draft {
        return Err(ReleaseError::NoPublishedRelease);
    }

    let mut assets = Vec::with_capacity(api.assets.len());
    for asset in api.assets {
        // An asset served from somewhere other than GitHub has no business in a
        // release, and this is the first point at which that can be caught.
        if AllowedHost::check(&asset.browser_download_url).is_err() {
            continue;
        }
        assets.push(Asset {
            name: asset.name,
            url: asset.browser_download_url,
            size: asset.size,
        });
    }

    Ok(Release {
        version: api.tag_name.trim_start_matches('v').to_owned(),
        assets,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn body(tag: &str, assets: &str) -> String {
        format!(r#"{{"tag_name":"{tag}","draft":false,"assets":[{assets}]}}"#)
    }

    fn api_asset(name: &str) -> String {
        format!(
            r#"{{"name":"{name}","size":12,"browser_download_url":"https://github.com/paar-git/panel-platform/releases/download/v0.1.0/{name}"}}"#
        )
    }

    #[test]
    fn the_leading_v_is_dropped_from_the_version() {
        let release = parse(&body("v0.1.0", "")).unwrap();
        assert_eq!(release.version, "0.1.0");
    }

    #[test]
    fn assets_keep_their_name_url_and_size() {
        let release = parse(&body("v0.1.0", &api_asset("Panel.deb"))).unwrap();

        let asset = release.assets.first().expect("one asset was parsed");
        assert_eq!(release.assets.len(), 1);
        assert_eq!(asset.name, "Panel.deb");
        assert_eq!(asset.size, 12);
        assert!(asset.url.starts_with("https://github.com/"));
    }

    /// The endpoint should never return one, so a draft arriving here means
    /// something is wrong and is refused rather than installed.
    #[test]
    fn a_draft_is_refused_even_if_the_endpoint_returns_one() {
        let body = r#"{"tag_name":"v0.1.0","draft":true,"assets":[]}"#;

        assert!(matches!(parse(body), Err(ReleaseError::NoPublishedRelease)));
    }

    /// An attacker who can influence the feed must not be able to point the
    /// download at a host of their choosing.
    #[test]
    fn an_asset_hosted_off_github_is_dropped() {
        let hostile = r#"{"name":"Panel.deb","size":12,"browser_download_url":"https://evil.example.com/Panel.deb"}"#;
        let release = parse(&body("v0.1.0", hostile)).unwrap();

        assert!(release.assets.is_empty());
    }

    #[test]
    fn plain_http_is_dropped_even_from_github() {
        let downgraded = r#"{"name":"Panel.deb","size":12,"browser_download_url":"http://github.com/x/y/Panel.deb"}"#;
        let release = parse(&body("v0.1.0", downgraded)).unwrap();

        assert!(release.assets.is_empty());
    }

    #[test]
    fn nonsense_is_reported_as_malformed_not_as_an_empty_release() {
        assert!(matches!(parse("not json"), Err(ReleaseError::Malformed(_))));
    }

    #[test]
    fn the_no_release_message_says_what_is_actually_true() {
        let message = ReleaseError::NoPublishedRelease.to_string();

        assert!(message.contains("no published release"));
        // The reader must not be sent looking for a broken network.
        assert!(!message.to_lowercase().contains("404"));
    }
}
