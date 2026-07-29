//! Deciding whether to offer an update.
//!
//! Separated from fetching and from installing so the rule that matters most —
//! *never move the user to a different version than the one they agreed to* —
//! is a pure function over a version, a release and some preferences.

use serde::{Deserialize, Serialize};

use crate::manifest::ValidatedRelease;

/// Which releases a user wants to be offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    #[default]
    Stable,
    /// Also offers pre-releases such as `1.4.0-beta.2`.
    Beta,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
        }
    }

    pub fn parse(text: &str) -> Option<Channel> {
        match text {
            "stable" => Some(Channel::Stable),
            "beta" => Some(Channel::Beta),
            _ => None,
        }
    }
}

/// What the user has asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdatePreferences {
    pub channel: Channel,
    /// Whether to look for updates without being asked. Checking contacts a
    /// remote server, so it is a thing the user can turn off — the application
    /// is otherwise able to run with no internet at all.
    pub check_automatically: bool,
    /// A version the user pressed "skip" on. Only that exact version is
    /// suppressed; a later one is still offered.
    pub skipped_version: Option<String>,
    /// Updates are never installed without a press of the button. This exists
    /// so the setting can be shown as deliberately unavailable rather than
    /// missing.
    pub install_automatically: bool,
}

impl Default for UpdatePreferences {
    fn default() -> Self {
        Self {
            channel: Channel::Stable,
            check_automatically: true,
            skipped_version: None,
            install_automatically: false,
        }
    }
}

/// An update the user may be offered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailableUpdate {
    pub current_version: String,
    pub new_version: String,
    pub notes: String,
    pub published_at: Option<String>,
    pub download_url: String,
    /// Carried through to the installer, which verifies it against the public
    /// key compiled into this build before running anything.
    pub signature: String,
}

/// The outcome of a check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UpdateCheck {
    UpToDate {
        current_version: String,
    },
    Available(AvailableUpdate),
    /// Offered before, and the user chose to skip this exact version.
    Skipped {
        skipped_version: String,
    },
    /// The published release is older than what is installed. Reported rather
    /// than silently treated as up to date, because it usually means a
    /// development build or a mistaken release.
    AheadOfPublished {
        current_version: String,
        published_version: String,
    },
}

impl UpdateCheck {
    pub fn available(&self) -> Option<&AvailableUpdate> {
        match self {
            UpdateCheck::Available(update) => Some(update),
            _ => None,
        }
    }

    /// What the window says. The user-facing string lives beside the decision
    /// so the two cannot drift.
    pub fn headline(&self) -> String {
        match self {
            UpdateCheck::Available(update) => {
                format!(
                    "There is an update available — version {}",
                    update.new_version
                )
            }
            UpdateCheck::UpToDate { current_version } => {
                format!("Panel Platform {current_version} is up to date")
            }
            UpdateCheck::Skipped { skipped_version } => {
                format!("Version {skipped_version} was skipped")
            }
            UpdateCheck::AheadOfPublished {
                current_version, ..
            } => format!("Running {current_version}, which is newer than the latest release"),
        }
    }
}

/// The same version with build metadata removed, for comparison only.
///
/// Never shown to the user or stored — `1.2.0+build.5` is still displayed and
/// recorded in full. This exists solely so two versions that differ only in
/// build metadata compare as the same release, which is what the SemVer
/// specification requires and what the `semver` crate does not do.
fn for_precedence(version: &semver::Version) -> semver::Version {
    semver::Version {
        build: semver::BuildMetadata::EMPTY,
        ..version.clone()
    }
}

/// Decide what to do about a release.
///
/// Only a strictly greater version is offered. An equal or lower published
/// version never is, so a feed that has been rolled back — or tampered with to
/// name an old, vulnerable version — cannot walk a user backwards.
pub fn decide(
    current: &semver::Version,
    release: &ValidatedRelease,
    preferences: &UpdatePreferences,
) -> UpdateCheck {
    // A pre-release is only ever offered on the beta channel, even if it sorts
    // higher. Semver already orders `1.4.0-beta.1` below `1.4.0`, but it sorts
    // *above* `1.3.0`, which is exactly how a stable user gets a beta by
    // accident.
    if !release.version.pre.is_empty() && preferences.channel != Channel::Beta {
        return UpdateCheck::UpToDate {
            current_version: current.to_string(),
        };
    }

    // Build metadata is stripped before comparing. The SemVer specification
    // (§10) says build metadata is ignored when determining precedence, but the
    // `semver` crate's `Ord` and `PartialEq` both take it into account —
    // `1.2.0+build.5` compares as *greater* than `1.2.0` there. Left alone,
    // that would offer `1.2.0+build.5` to a user already running `1.2.0`;
    // installing it would change nothing, so the offer would return forever.
    match for_precedence(&release.version).cmp(&for_precedence(current)) {
        std::cmp::Ordering::Less => {
            return UpdateCheck::AheadOfPublished {
                current_version: current.to_string(),
                published_version: release.version.to_string(),
            }
        }
        std::cmp::Ordering::Equal => {
            return UpdateCheck::UpToDate {
                current_version: current.to_string(),
            }
        }
        std::cmp::Ordering::Greater => {}
    }

    if preferences.skipped_version.as_deref() == Some(release.version.to_string().as_str()) {
        return UpdateCheck::Skipped {
            skipped_version: release.version.to_string(),
        };
    }

    UpdateCheck::Available(AvailableUpdate {
        current_version: current.to_string(),
        new_version: release.version.to_string(),
        notes: release.notes.clone(),
        published_at: release.pub_date.clone(),
        download_url: release.url.clone(),
        signature: release.signature.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(version: &str) -> ValidatedRelease {
        ValidatedRelease {
            version: semver::Version::parse(version).expect("valid version"),
            notes: "Some notes.".to_string(),
            pub_date: Some("2026-08-01T00:00:00Z".to_string()),
            url: "https://github.com/x/y/app.msi".to_string(),
            signature: "a-signature".to_string(),
        }
    }

    fn version(text: &str) -> semver::Version {
        semver::Version::parse(text).expect("valid version")
    }

    #[test]
    fn a_newer_release_is_offered() {
        let check = decide(
            &version("1.2.0"),
            &release("1.3.0"),
            &UpdatePreferences::default(),
        );
        let update = check.available().expect("should be offered");
        assert_eq!(update.new_version, "1.3.0");
        assert_eq!(update.current_version, "1.2.0");
        assert!(check.headline().contains("There is an update available"));
        assert!(check.headline().contains("1.3.0"));
    }

    #[test]
    fn the_same_version_is_up_to_date() {
        let check = decide(
            &version("1.2.0"),
            &release("1.2.0"),
            &UpdatePreferences::default(),
        );
        assert_eq!(
            check,
            UpdateCheck::UpToDate {
                current_version: "1.2.0".to_string()
            }
        );
    }

    #[test]
    fn an_older_published_release_never_walks_the_user_backwards() {
        // The dangerous case: a rolled-back or tampered manifest naming an old
        // version with a known problem in it.
        let check = decide(
            &version("1.5.0"),
            &release("1.2.0"),
            &UpdatePreferences::default(),
        );
        assert!(
            check.available().is_none(),
            "a downgrade must never be offered"
        );
        assert!(matches!(check, UpdateCheck::AheadOfPublished { .. }));
    }

    #[test]
    fn a_prerelease_is_not_offered_on_the_stable_channel() {
        // `1.4.0-beta.1` sorts above `1.3.0`, so a naive comparison hands every
        // stable user a beta.
        let check = decide(
            &version("1.3.0"),
            &release("1.4.0-beta.1"),
            &UpdatePreferences::default(),
        );
        assert!(
            check.available().is_none(),
            "stable users get stable builds"
        );
    }

    #[test]
    fn a_prerelease_is_offered_on_the_beta_channel() {
        let preferences = UpdatePreferences {
            channel: Channel::Beta,
            ..UpdatePreferences::default()
        };
        let check = decide(&version("1.3.0"), &release("1.4.0-beta.1"), &preferences);
        assert_eq!(
            check.available().expect("offered").new_version,
            "1.4.0-beta.1"
        );
    }

    #[test]
    fn a_stable_release_is_still_offered_to_a_beta_user() {
        let preferences = UpdatePreferences {
            channel: Channel::Beta,
            ..UpdatePreferences::default()
        };
        let check = decide(&version("1.3.0"), &release("1.4.0"), &preferences);
        assert!(check.available().is_some());
    }

    #[test]
    fn a_skipped_version_is_not_offered_again() {
        let preferences = UpdatePreferences {
            skipped_version: Some("1.3.0".to_string()),
            ..UpdatePreferences::default()
        };
        let check = decide(&version("1.2.0"), &release("1.3.0"), &preferences);
        assert_eq!(
            check,
            UpdateCheck::Skipped {
                skipped_version: "1.3.0".to_string()
            }
        );
    }

    #[test]
    fn skipping_one_version_does_not_skip_the_next() {
        // Otherwise "skip" quietly becomes "never update again".
        let preferences = UpdatePreferences {
            skipped_version: Some("1.3.0".to_string()),
            ..UpdatePreferences::default()
        };
        let check = decide(&version("1.2.0"), &release("1.4.0"), &preferences);
        assert_eq!(check.available().expect("offered").new_version, "1.4.0");
    }

    #[test]
    fn the_offer_carries_the_signature_through_to_the_installer() {
        // If this were dropped, the install step would have nothing to verify
        // the download against.
        let check = decide(
            &version("1.2.0"),
            &release("1.3.0"),
            &UpdatePreferences::default(),
        );
        let update = check.available().expect("offered");
        assert_eq!(update.signature, "a-signature");
        assert!(update.download_url.starts_with("https://"));
    }

    #[test]
    fn updates_are_never_installed_automatically_by_default() {
        // The button is the consent. An updater that replaced a running host's
        // software unprompted would be a surprise at the worst moment.
        assert!(!UpdatePreferences::default().install_automatically);
        assert!(UpdatePreferences::default().check_automatically);
        assert_eq!(UpdatePreferences::default().channel, Channel::Stable);
    }

    #[test]
    fn a_channel_round_trips_through_its_stored_name() {
        for channel in [Channel::Stable, Channel::Beta] {
            assert_eq!(Channel::parse(channel.as_str()), Some(channel));
        }
        assert_eq!(Channel::parse("nightly"), None);
    }

    #[test]
    fn every_outcome_has_something_to_show_the_user() {
        let outcomes = [
            decide(
                &version("1.2.0"),
                &release("1.3.0"),
                &UpdatePreferences::default(),
            ),
            decide(
                &version("1.2.0"),
                &release("1.2.0"),
                &UpdatePreferences::default(),
            ),
            decide(
                &version("1.5.0"),
                &release("1.2.0"),
                &UpdatePreferences::default(),
            ),
            decide(
                &version("1.2.0"),
                &release("1.3.0"),
                &UpdatePreferences {
                    skipped_version: Some("1.3.0".to_string()),
                    ..UpdatePreferences::default()
                },
            ),
        ];
        for outcome in outcomes {
            assert!(
                !outcome.headline().is_empty(),
                "{outcome:?} has no headline"
            );
        }
    }

    #[test]
    fn build_metadata_does_not_make_a_version_newer() {
        // `1.2.0+build.5` and `1.2.0` are the same release by semver's rules,
        // and offering one as an update to the other would loop forever.
        let check = decide(
            &version("1.2.0"),
            &release("1.2.0+build.5"),
            &UpdatePreferences::default(),
        );
        assert!(check.available().is_none(), "{check:?}");
    }
}
