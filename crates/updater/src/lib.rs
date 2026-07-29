//! Finding out that a new release exists, and deciding whether to offer it.
//!
//! The flow the user sees is three steps, and this crate owns the first two:
//!
//! 1. The application asks the release feed what the latest version is.
//! 2. It decides whether that version should be offered, producing
//!    *"There is an update available"* or nothing at all.
//! 3. The user presses **Update now**, and the download is verified against a
//!    signing key and installed.
//!
//! **Step 3 is not in this crate and is not yet built.** It belongs to the
//! desktop shell, because replacing a running application is something only the
//! shell can do — on Windows a running `.exe` cannot overwrite itself, so the
//! installer has to take over and restart it. That is Tauri's updater plugin's
//! job, and `apps/desktop` does not exist yet.
//!
//! # Why this is the most safety-critical input in the application
//!
//! Everything else that arrives from outside is validated and then *stored*.
//! This is validated and then *executed*. The controls that follow from that
//! live in [`manifest`]:
//!
//! * downloads must be `https` from an allowed host, so a tampered manifest
//!   cannot redirect the installer;
//! * a release must be signed, and the signature is checked against a key
//!   compiled into the binary — not one supplied by the feed;
//! * a version older than or equal to the installed one is never offered, so a
//!   rolled-back feed cannot walk a user back onto an old build.
//!
//! Nothing installs without the button being pressed. See
//! [`check::UpdatePreferences::install_automatically`].

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod check;
pub mod manifest;

pub use check::{decide, AvailableUpdate, Channel, UpdateCheck, UpdatePreferences};
pub use manifest::{
    current_target, parse_version, ManifestError, PlatformBuild, ReleaseManifest, ValidatedRelease,
    ALLOWED_HOSTS,
};

/// The version of the running build, from the crate metadata.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where releases are published.
///
/// A constant rather than a setting: a configurable update feed is a
/// configurable way to install someone else's software.
pub const RELEASE_FEED_URL: &str =
    "https://github.com/realpargitDEV/project-host/releases/latest/download/latest.json";

/// Check a fetched manifest against the running version.
///
/// Fetching is the caller's job, so this crate stays free of an HTTP client and
/// every rule in it remains testable without a network.
pub fn evaluate(
    manifest: &ReleaseManifest,
    current: &str,
    preferences: &UpdatePreferences,
) -> Result<UpdateCheck, ManifestError> {
    let current = parse_version(current)?;
    let release = manifest.validate_for(current_target())?;
    Ok(decide(&current, &release, preferences))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn the_running_version_parses_as_semantic_versioning() {
        // If the crate version were ever set to something semver cannot read,
        // every update check would fail at the first step.
        assert!(parse_version(CURRENT_VERSION).is_ok(), "{CURRENT_VERSION}");
    }

    #[test]
    fn the_release_feed_is_an_https_url_on_an_allowed_host() {
        assert!(RELEASE_FEED_URL.starts_with("https://"));
        assert!(
            ALLOWED_HOSTS
                .iter()
                .any(|host| RELEASE_FEED_URL.contains(host)),
            "the feed itself must live somewhere downloads are allowed from"
        );
    }

    #[test]
    fn a_newer_release_for_this_platform_is_offered() {
        let mut platforms = BTreeMap::new();
        platforms.insert(
            current_target().to_string(),
            PlatformBuild {
                url: "https://github.com/x/y/app".to_string(),
                signature: "sig".to_string(),
            },
        );
        let manifest = ReleaseManifest {
            version: "99.0.0".to_string(),
            notes: "The future.".to_string(),
            pub_date: None,
            platforms,
        };

        let check =
            evaluate(&manifest, CURRENT_VERSION, &UpdatePreferences::default()).expect("evaluates");
        assert_eq!(check.available().expect("offered").new_version, "99.0.0");
    }

    #[test]
    fn a_release_with_no_build_for_this_platform_is_an_error_not_an_offer() {
        let manifest = ReleaseManifest {
            version: "99.0.0".to_string(),
            notes: String::new(),
            pub_date: None,
            platforms: BTreeMap::new(),
        };
        assert_eq!(
            evaluate(&manifest, CURRENT_VERSION, &UpdatePreferences::default()),
            Err(ManifestError::NoPlatformBuild)
        );
    }
}
