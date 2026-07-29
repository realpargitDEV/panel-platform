//! The control panel: what it offers, and how a click gets back to us.
//!
//! Discord identifies a pressed button by echoing back the `custom_id` string
//! that was set when the message was built. That string is the only context an
//! interaction carries, and it has three properties worth taking seriously:
//!
//! * It is **untrusted**. Discord echoes it faithfully, but the message it came
//!   from may be months old, and nothing stops a crafted interaction from a
//!   different source presenting whatever it likes. It is parsed strictly and
//!   the identifiers inside it are re-validated.
//! * It is **capped at 100 characters** by Discord. A panel built for a project
//!   whose id and feature key together overflow that would be silently rejected
//!   at send time, so encoding fails loudly instead.
//! * It carries **no permission**. Which button was drawn says nothing about
//!   who is allowed to press it; see [`crate::permissions`].

use serde::{Deserialize, Serialize};

use project_host_api_types::ids::ProjectId;

use crate::permissions::Action;

/// Discord's hard limit on a component `custom_id`.
pub const MAX_CUSTOM_ID_LENGTH: usize = 100;

/// Version tag opening every custom id.
///
/// Panels persist in Discord channels indefinitely. When the encoding changes,
/// old messages will still be clicked, and this is what lets the handler tell
/// "an id from a previous version" from "not ours at all" and answer the user
/// with "this panel is out of date, here is a fresh one".
const PREFIX: &str = "ph1";

/// How long a feature key inside a custom id may be.
///
/// Chosen so that the longest possible id — prefix, the longest action name, a
/// project id and a maximal key — still fits within Discord's limit.
pub const MAX_FEATURE_KEY_LENGTH: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CustomIdError {
    #[error("not a Project Host control (missing the `{PREFIX}` prefix)")]
    NotOurs,
    #[error("this control was made by a different version of the application")]
    WrongVersion,
    #[error("malformed control id")]
    Malformed,
    #[error("unknown action `{0}`")]
    UnknownAction(String),
    #[error("invalid project id: {0}")]
    BadProject(#[from] project_host_api_types::ids::IdError),
    #[error("this action takes no argument")]
    UnexpectedArgument,
    #[error("this action requires an argument")]
    MissingArgument,
    #[error("invalid feature key `{0}`")]
    BadFeatureKey(String),
    #[error("control id is {length} characters; Discord allows {MAX_CUSTOM_ID_LENGTH}")]
    TooLong { length: usize },
}

/// A decoded button or select-menu identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Control {
    pub action: Action,
    pub project: ProjectId,
    /// Present only for [`Action::ToggleFeature`], naming which feature.
    pub feature: Option<String>,
}

impl Control {
    pub fn new(action: Action, project: ProjectId) -> Result<Self, CustomIdError> {
        if action == Action::ToggleFeature {
            return Err(CustomIdError::MissingArgument);
        }
        Ok(Self {
            action,
            project,
            feature: None,
        })
    }

    pub fn toggle_feature(project: ProjectId, feature: &str) -> Result<Self, CustomIdError> {
        validate_feature_key(feature)?;
        Ok(Self {
            action: Action::ToggleFeature,
            project,
            feature: Some(feature.to_string()),
        })
    }

    /// Render this control as a `custom_id`.
    ///
    /// Fails rather than truncating. A truncated id would decode to a different
    /// project, or to nothing, once a user pressed it.
    pub fn encode(&self) -> Result<String, CustomIdError> {
        let mut encoded = format!("{PREFIX}:{}:{}", self.action.as_str(), self.project);
        if let Some(feature) = &self.feature {
            validate_feature_key(feature)?;
            encoded.push(':');
            encoded.push_str(feature);
        }

        if encoded.len() > MAX_CUSTOM_ID_LENGTH {
            return Err(CustomIdError::TooLong {
                length: encoded.len(),
            });
        }
        Ok(encoded)
    }

    /// Parse a `custom_id` that arrived on an interaction.
    ///
    /// Strict on every field. Anything unrecognised is refused rather than
    /// defaulted, because the only thing on the other side of a wrong guess is
    /// performing an action on a project the user did not name.
    pub fn decode(raw: &str) -> Result<Self, CustomIdError> {
        // Length is checked first: Discord will not deliver an over-long id, so
        // one arriving here means the input did not come from a panel we built.
        if raw.len() > MAX_CUSTOM_ID_LENGTH {
            return Err(CustomIdError::TooLong { length: raw.len() });
        }

        let mut parts = raw.split(':');

        match parts.next() {
            Some(PREFIX) => {}
            // A tag shaped like ours but numbered differently is a stale panel,
            // which deserves a different message than a foreign component.
            Some(other) if other.starts_with("ph") && other.len() <= 4 => {
                return Err(CustomIdError::WrongVersion)
            }
            _ => return Err(CustomIdError::NotOurs),
        }

        let action_text = parts.next().ok_or(CustomIdError::Malformed)?;
        let action = Action::parse(action_text)
            .ok_or_else(|| CustomIdError::UnknownAction(action_text.to_string()))?;

        let project_text = parts.next().ok_or(CustomIdError::Malformed)?;
        let project = ProjectId::parse(project_text)?;

        let argument = parts.next();
        // A fourth colon means the argument contained a separator, which
        // `validate_feature_key` would reject anyway — but catching it here
        // keeps the error honest about what was wrong.
        if parts.next().is_some() {
            return Err(CustomIdError::Malformed);
        }

        match (action, argument) {
            (Action::ToggleFeature, Some(feature)) => {
                validate_feature_key(feature)?;
                Ok(Self {
                    action,
                    project,
                    feature: Some(feature.to_string()),
                })
            }
            (Action::ToggleFeature, None) => Err(CustomIdError::MissingArgument),
            (_, Some(_)) => Err(CustomIdError::UnexpectedArgument),
            (_, None) => Ok(Self {
                action,
                project,
                feature: None,
            }),
        }
    }
}

/// Feature keys are ours, not free text: they name a switch the project
/// defines. Restricting the alphabet keeps them safe inside a colon-separated
/// id and safe to echo back into a Discord message.
fn validate_feature_key(key: &str) -> Result<(), CustomIdError> {
    let acceptable = !key.is_empty()
        && key.len() <= MAX_FEATURE_KEY_LENGTH
        && key
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');

    if acceptable {
        Ok(())
    } else {
        Err(CustomIdError::BadFeatureKey(key.to_string()))
    }
}

/// One button on the control panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelButton {
    pub label: &'static str,
    pub action: Action,
    pub style: ButtonStyle,
    /// Whether pressing it should ask for confirmation first.
    pub confirm: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ButtonStyle {
    Primary,
    Secondary,
    Success,
    Danger,
}

/// The buttons a control panel shows, in display order.
///
/// Discord allows five buttons per action row and five rows per message. This
/// list is deliberately eight, leaving room for the feature select menu below
/// it without approaching either limit.
pub const PANEL_BUTTONS: &[PanelButton] = &[
    PanelButton {
        label: "Start",
        action: Action::Start,
        style: ButtonStyle::Success,
        confirm: false,
    },
    PanelButton {
        label: "Stop",
        action: Action::Stop,
        style: ButtonStyle::Danger,
        confirm: true,
    },
    PanelButton {
        label: "Restart",
        action: Action::Restart,
        style: ButtonStyle::Danger,
        confirm: true,
    },
    PanelButton {
        label: "Status",
        action: Action::Status,
        style: ButtonStyle::Primary,
        confirm: false,
    },
    PanelButton {
        label: "Resources",
        action: Action::Resources,
        style: ButtonStyle::Secondary,
        confirm: false,
    },
    PanelButton {
        label: "Recent errors",
        action: Action::RecentErrors,
        style: ButtonStyle::Secondary,
        confirm: false,
    },
    PanelButton {
        label: "Logs",
        action: Action::Logs,
        style: ButtonStyle::Secondary,
        confirm: false,
    },
    PanelButton {
        label: "Run scan",
        action: Action::Scan,
        style: ButtonStyle::Secondary,
        confirm: false,
    },
];

/// Discord's limit on buttons in one action row.
pub const BUTTONS_PER_ROW: usize = 5;

/// Split the panel buttons into Discord action rows.
pub fn button_rows() -> Vec<&'static [PanelButton]> {
    PANEL_BUTTONS.chunks(BUTTONS_PER_ROW).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> ProjectId {
        ProjectId::generate()
    }

    #[test]
    fn a_control_round_trips_through_its_custom_id() {
        for action in Action::ALL {
            if *action == Action::ToggleFeature {
                continue;
            }
            let control = Control::new(*action, project()).expect("buildable");
            let encoded = control.encode().expect("encodes");
            assert_eq!(Control::decode(&encoded).expect("decodes"), control);
        }
    }

    #[test]
    fn a_feature_toggle_round_trips_with_its_key() {
        let control = Control::toggle_feature(project(), "auto_restart").expect("valid key");
        let encoded = control.encode().expect("encodes");
        let decoded = Control::decode(&encoded).expect("decodes");
        assert_eq!(decoded.action, Action::ToggleFeature);
        assert_eq!(decoded.feature.as_deref(), Some("auto_restart"));
    }

    #[test]
    fn every_encodable_control_fits_discords_limit() {
        // The check that keeps a panel from failing to send at runtime.
        let longest_key = "x".repeat(MAX_FEATURE_KEY_LENGTH);
        let control = Control::toggle_feature(project(), &longest_key).expect("valid key");
        let encoded = control.encode().expect("must fit");
        assert!(
            encoded.len() <= MAX_CUSTOM_ID_LENGTH,
            "worst case is {} characters",
            encoded.len()
        );

        for action in Action::ALL {
            if *action == Action::ToggleFeature {
                continue;
            }
            let encoded = Control::new(*action, project())
                .expect("buildable")
                .encode()
                .expect("must fit");
            assert!(encoded.len() <= MAX_CUSTOM_ID_LENGTH);
        }
    }

    #[test]
    fn a_foreign_custom_id_is_refused() {
        for foreign in ["", "confirm", "other-bot:start", "ph:start", ":::"] {
            assert!(
                Control::decode(foreign).is_err(),
                "{foreign:?} should not decode"
            );
        }
    }

    #[test]
    fn a_stale_panel_is_distinguished_from_a_foreign_component() {
        // So the user is told to refresh the panel, not that the bot is broken.
        let error = Control::decode("ph0:start:prj_x").expect_err("old version");
        assert_eq!(error, CustomIdError::WrongVersion);

        let error = Control::decode("someoneelse:start:prj_x").expect_err("foreign");
        assert_eq!(error, CustomIdError::NotOurs);
    }

    #[test]
    fn an_unknown_action_is_refused_rather_than_ignored() {
        let id = format!("{PREFIX}:delete_everything:{}", project());
        assert!(matches!(
            Control::decode(&id),
            Err(CustomIdError::UnknownAction(_))
        ));
    }

    #[test]
    fn a_malformed_project_id_is_refused() {
        // The important one: this string decides which project gets stopped.
        for bad in [
            "prj_not-a-uuid",
            "usr_0193000000007000800000000000abcd",
            "prj_",
            "../../etc/passwd",
        ] {
            let id = format!("{PREFIX}:stop:{bad}");
            assert!(Control::decode(&id).is_err(), "{bad:?} should not decode");
        }
    }

    #[test]
    fn an_action_that_takes_no_argument_refuses_one() {
        // Otherwise a crafted id could smuggle a payload past the handler into
        // whatever eventually reads `feature`.
        let id = format!("{PREFIX}:stop:{}:injected", project());
        assert_eq!(Control::decode(&id), Err(CustomIdError::UnexpectedArgument));
    }

    #[test]
    fn a_feature_toggle_without_a_key_is_refused() {
        let id = format!("{PREFIX}:toggle_feature:{}", project());
        assert_eq!(Control::decode(&id), Err(CustomIdError::MissingArgument));
        assert!(matches!(
            Control::new(Action::ToggleFeature, project()),
            Err(CustomIdError::MissingArgument)
        ));
    }

    #[test]
    fn a_feature_key_outside_the_allowed_alphabet_is_refused() {
        let long = "x".repeat(MAX_FEATURE_KEY_LENGTH + 1);
        for bad in [
            "",
            "Upper",
            "with space",
            "with-dash",
            "semi;colon",
            "unicode_é",
            long.as_str(),
        ] {
            assert!(
                Control::toggle_feature(project(), bad).is_err(),
                "{bad:?} should not be a valid feature key"
            );
        }
    }

    #[test]
    fn an_over_long_custom_id_is_refused_before_it_is_parsed() {
        let id = format!("{PREFIX}:stop:{}", "a".repeat(MAX_CUSTOM_ID_LENGTH));
        assert!(matches!(
            Control::decode(&id),
            Err(CustomIdError::TooLong { .. })
        ));
    }

    #[test]
    fn destructive_buttons_ask_for_confirmation() {
        for button in PANEL_BUTTONS {
            assert_eq!(
                button.confirm,
                button.action.is_destructive(),
                "{:?} confirmation should match whether it is destructive",
                button.action
            );
        }
    }

    #[test]
    fn the_panel_fits_discords_action_rows() {
        // Five buttons per row, five rows per message, and one row is reserved
        // for the feature select menu.
        let rows = button_rows();
        assert!(
            rows.len() <= 4,
            "{} rows leaves no room for the menu",
            rows.len()
        );
        for row in rows {
            assert!(row.len() <= BUTTONS_PER_ROW);
        }
    }

    #[test]
    fn the_panel_offers_no_action_a_viewer_could_not_be_shown() {
        // Every button must map to an action the permission model knows, or
        // authorisation would have nothing to check it against.
        for button in PANEL_BUTTONS {
            assert!(Action::ALL.contains(&button.action));
        }
    }
}
