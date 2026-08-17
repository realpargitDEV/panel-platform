//! The release manifest, and refusing to trust a bad one.
//!
//! This is the most dangerous input the application accepts. Everything else
//! that arrives from outside gets validated and then *stored*; this one gets
//! validated and then **executed**. A manifest that can point the "Update now"
//! button at an arbitrary URL is a remote code execution channel into the
//! user's machine.
//!
//! So a manifest is refused unless all of the following hold:
//!
//! * the download URL is `https`, never `http`;
//! * its host is on [`ALLOWED_HOSTS`], so a tampered manifest cannot redirect
//!   the download somewhere else even if the manifest itself was served
//!   legitimately;
//! * a signature is present, because the download is verified against the
//!   public key compiled into the binary before anything is run;
//! * the version parses as semantic versioning.
//!
//! The signature check itself happens at install time in the desktop shell,
//! using the key baked into the build. Nothing here can substitute for it —
//! this layer decides whether a download is even worth attempting.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Hosts a release binary may be downloaded from.
///
/// GitHub serves release assets from `objects.githubusercontent.com` after a
/// redirect, so both appear. Adding to this list means widening what a
/// compromised manifest can reach, and should be done deliberately.
pub const ALLOWED_HOSTS: &[&str] = &[
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    #[error("`{0}` is not a version this application understands")]
    BadVersion(String),
    #[error("the release has no download for this platform")]
    NoPlatformBuild,
    #[error("a release download must use https, got `{0}`")]
    NotHttps(String),
    #[error("`{0}` is not a host releases may be downloaded from")]
    HostNotAllowed(String),
    #[error("the download url `{0}` could not be understood")]
    MalformedUrl(String),
    #[error("the release is not signed")]
    Unsigned,
    #[error("release notes are longer than {limit} characters")]
    NotesTooLong { limit: usize },
}

/// Release notes are shown in a dialog, so they are bounded.
pub const MAX_NOTES_LENGTH: usize = 20_000;

/// One platform's build of a release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformBuild {
    pub url: String,
    /// Detached signature over the download, verified before install.
    pub signature: String,
}

/// A release, as published.
///
/// The shape matches what Tauri's updater expects, so the same document can
/// serve both this check and the install step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseManifest {
    pub version: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub pub_date: Option<String>,
    /// Keyed by target triple, e.g. `windows-x86_64`.
    pub platforms: BTreeMap<String, PlatformBuild>,
}

/// A validated release for the platform we are running on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedRelease {
    pub version: semver::Version,
    pub notes: String,
    pub pub_date: Option<String>,
    pub url: String,
    pub signature: String,
}

impl ReleaseManifest {
    /// Check a manifest and extract the build for `target`.
    pub fn validate_for(&self, target: &str) -> Result<ValidatedRelease, ManifestError> {
        let version = parse_version(&self.version)?;

        if self.notes.len() > MAX_NOTES_LENGTH {
            return Err(ManifestError::NotesTooLong {
                limit: MAX_NOTES_LENGTH,
            });
        }

        let build = self
            .platforms
            .get(target)
            .ok_or(ManifestError::NoPlatformBuild)?;

        validate_url(&build.url)?;

        if build.signature.trim().is_empty() {
            return Err(ManifestError::Unsigned);
        }

        Ok(ValidatedRelease {
            version,
            notes: self.notes.clone(),
            pub_date: self.pub_date.clone(),
            url: build.url.clone(),
            signature: build.signature.clone(),
        })
    }
}

/// Parse a version, tolerating a leading `v` because release tags usually have
/// one and the manifest is written by hand often enough to matter.
pub fn parse_version(text: &str) -> Result<semver::Version, ManifestError> {
    let trimmed = text.trim().trim_start_matches('v');
    semver::Version::parse(trimmed).map_err(|_| ManifestError::BadVersion(text.to_string()))
}

/// Reject anything that is not an `https` URL on an allowed host.
///
/// Parsed by hand rather than with a URL crate: the check is small, and the
/// part that matters — extracting the host without being fooled by userinfo,
/// which is how `https://github.com@evil.example/` gets past naive checks — is
/// clearer written out than delegated.
fn validate_url(url: &str) -> Result<(), ManifestError> {
    let Some(rest) = url.strip_prefix("https://") else {
        // Named specifically so the error can say "http" rather than a generic
        // "malformed", because that is the mistake a person actually makes.
        let scheme = url.split("://").next().unwrap_or(url);
        return Err(ManifestError::NotHttps(scheme.to_string()));
    };

    // The authority ends at the first `/`, `?` or `#`.
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .filter(|part| !part.is_empty())
        .ok_or_else(|| ManifestError::MalformedUrl(url.to_string()))?;

    // Anything before an `@` is userinfo, and the host is what follows it.
    // `https://github.com@evil.example/x` has a host of `evil.example`.
    let host_and_port = match authority.rsplit_once('@') {
        Some((_userinfo, host)) => host,
        None => authority,
    };

    // Strip a port. An IPv6 literal would be bracketed; none of the allowed
    // hosts are addresses, so a bracket is itself disqualifying.
    if host_and_port.starts_with('[') {
        return Err(ManifestError::HostNotAllowed(host_and_port.to_string()));
    }
    let host = host_and_port
        .split(':')
        .next()
        .filter(|part| !part.is_empty())
        .ok_or_else(|| ManifestError::MalformedUrl(url.to_string()))?;

    let host_lower = host.to_ascii_lowercase();
    if !ALLOWED_HOSTS.contains(&host_lower.as_str()) {
        return Err(ManifestError::HostNotAllowed(host.to_string()));
    }

    Ok(())
}

/// The target triple for the running build, in the form the manifest uses.
pub fn current_target() -> &'static str {
    // Written as explicit cfg arms rather than assembled from `std::env::consts`
    // so that an unsupported platform is a compile-time gap rather than a
    // string that silently matches nothing in the manifest.
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "windows-x86_64"
    }
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    {
        "windows-aarch64"
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "linux-x86_64"
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "linux-aarch64"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "darwin-x86_64"
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "darwin-aarch64"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(url: &str) -> ReleaseManifest {
        let mut platforms = BTreeMap::new();
        platforms.insert(
            "windows-x86_64".to_string(),
            PlatformBuild {
                url: url.to_string(),
                signature: "dW50cnVzdGVkIGNvbW1lbnQ6...".to_string(),
            },
        );
        ReleaseManifest {
            version: "1.2.0".to_string(),
            notes: "Fixed a thing.".to_string(),
            pub_date: Some("2026-08-01T00:00:00Z".to_string()),
            platforms,
        }
    }

    #[test]
    fn a_well_formed_release_validates() {
        let release =
            manifest("https://github.com/paar-git/panel-platform/releases/download/v1.2.0/app.msi")
                .validate_for("windows-x86_64")
                .expect("valid");
        assert_eq!(release.version, semver::Version::new(1, 2, 0));
        assert!(!release.signature.is_empty());
    }

    #[test]
    fn a_plain_http_download_is_refused() {
        // The whole point: an attacker on the network could otherwise replace
        // the installer in flight.
        let error = manifest("http://github.com/x/y/app.msi")
            .validate_for("windows-x86_64")
            .expect_err("http must be refused");
        assert!(matches!(error, ManifestError::NotHttps(_)));
    }

    #[test]
    fn a_download_from_an_unexpected_host_is_refused() {
        // Even over https, and even if the manifest itself was served from the
        // right place.
        for hostile in [
            "https://evil.example/app.msi",
            "https://github.com.evil.example/app.msi",
            "https://notgithub.com/app.msi",
        ] {
            let error = manifest(hostile)
                .validate_for("windows-x86_64")
                .expect_err("{hostile} must be refused");
            assert!(
                matches!(error, ManifestError::HostNotAllowed(_)),
                "{hostile} gave {error:?}"
            );
        }
    }

    #[test]
    fn userinfo_cannot_disguise_the_real_host() {
        // `https://github.com@evil.example/` is a URL whose host is
        // evil.example. A check that looked for "github.com" anywhere in the
        // string would wave this through.
        let error = manifest("https://github.com@evil.example/app.msi")
            .validate_for("windows-x86_64")
            .expect_err("must look past the userinfo");
        assert_eq!(
            error,
            ManifestError::HostNotAllowed("evil.example".to_string())
        );
    }

    #[test]
    fn a_password_in_the_userinfo_does_not_help_either() {
        let error = manifest("https://user:github.com@evil.example/app.msi")
            .validate_for("windows-x86_64")
            .expect_err("must look past the userinfo");
        assert!(matches!(error, ManifestError::HostNotAllowed(_)));
    }

    #[test]
    fn an_allowed_host_with_a_port_is_still_matched_by_host() {
        assert!(manifest("https://github.com:443/x/app.msi")
            .validate_for("windows-x86_64")
            .is_ok());
    }

    #[test]
    fn host_matching_is_case_insensitive() {
        assert!(manifest("https://GitHub.COM/x/app.msi")
            .validate_for("windows-x86_64")
            .is_ok());
    }

    #[test]
    fn an_ip_literal_is_refused() {
        for literal in ["https://127.0.0.1/app.msi", "https://[::1]/app.msi"] {
            assert!(
                manifest(literal).validate_for("windows-x86_64").is_err(),
                "{literal} should be refused"
            );
        }
    }

    #[test]
    fn an_unsigned_release_is_refused() {
        // Without a signature there is nothing to check the download against,
        // and installing it would be running whatever arrived.
        let mut bad = manifest("https://github.com/x/app.msi");
        if let Some(build) = bad.platforms.get_mut("windows-x86_64") {
            build.signature = "   ".to_string();
        }
        assert_eq!(
            bad.validate_for("windows-x86_64"),
            Err(ManifestError::Unsigned)
        );
    }

    #[test]
    fn a_release_without_a_build_for_this_platform_is_reported_as_such() {
        // Not an error the user should see as "update failed" — there simply is
        // no build for them yet.
        assert_eq!(
            manifest("https://github.com/x/app.msi").validate_for("linux-aarch64"),
            Err(ManifestError::NoPlatformBuild)
        );
    }

    #[test]
    fn a_version_that_is_not_semantic_is_refused() {
        for bad in ["", "latest", "1", "1.2", "2026-08-01", "v"] {
            let mut broken = manifest("https://github.com/x/app.msi");
            broken.version = bad.to_string();
            assert!(
                broken.validate_for("windows-x86_64").is_err(),
                "{bad:?} should not parse as a version"
            );
        }
    }

    #[test]
    fn a_leading_v_on_a_tag_is_tolerated() {
        // Release tags are written `v1.2.0` far more often than `1.2.0`.
        assert_eq!(
            parse_version("v1.2.0").expect("parses"),
            semver::Version::new(1, 2, 0)
        );
    }

    #[test]
    fn enormous_release_notes_are_refused() {
        // They go into a dialog; an unbounded string is a way to make the
        // update prompt unusable.
        let mut huge = manifest("https://github.com/x/app.msi");
        huge.notes = "x".repeat(MAX_NOTES_LENGTH + 1);
        assert!(matches!(
            huge.validate_for("windows-x86_64"),
            Err(ManifestError::NotesTooLong { .. })
        ));
    }

    #[test]
    fn the_current_target_is_one_the_manifest_could_name() {
        let target = current_target();
        assert!(target.contains('-'), "got {target}");
        assert!(!target.is_empty());
    }

    #[test]
    fn a_real_manifest_document_deserialises() {
        let json = r#"{
            "version": "1.3.0",
            "notes": "Adds Discord control panels.",
            "pub_date": "2026-08-01T12:00:00Z",
            "platforms": {
                "windows-x86_64": {
                    "url": "https://github.com/paar-git/panel-platform/releases/download/v1.3.0/ProjectHost_1.3.0_x64.msi",
                    "signature": "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZQ=="
                }
            }
        }"#;

        let manifest: ReleaseManifest = serde_json::from_str(json).expect("deserialise");
        let release = manifest.validate_for("windows-x86_64").expect("valid");
        assert_eq!(release.version, semver::Version::new(1, 3, 0));
    }
}
