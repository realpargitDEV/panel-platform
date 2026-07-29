//! Channel naming.
//!
//! Two dedicated channels are created per project: one for logs and events, one
//! for the control panel. Their names are customisable, which makes them user
//! input, which means the same rule that governs project directories applies
//! here: **a name is a label, never an identifier**.
//!
//! Everything that acts on a channel does so through the stored channel id that
//! Discord returned when it was created. Nothing in this crate ever looks a
//! channel up by name — otherwise renaming a channel, or a second channel with
//! a colliding name, would silently redirect a project's logs.

use serde::{Deserialize, Serialize};

use crate::snowflake::Snowflake;

/// Discord's limit on a channel name.
pub const MAX_CHANNEL_NAME_LENGTH: usize = 100;

/// Used when sanitising leaves nothing usable — a template of `{{}}` filled
/// with a slug of all-emoji, say. A channel with a dull name is recoverable;
/// a failed project creation because a name could not be produced is not.
const FALLBACK_NAME: &str = "project";

/// Which of a project's two channels this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    /// Logs, errors, warnings, deployments and status changes.
    Logs,
    /// The control panel message and its buttons.
    Control,
}

impl ChannelKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ChannelKind::Logs => "logs",
            ChannelKind::Control => "control",
        }
    }

    pub fn default_template(self) -> &'static str {
        match self {
            ChannelKind::Logs => "{slug}-logs",
            ChannelKind::Control => "{slug}-control",
        }
    }

    pub const ALL: &'static [ChannelKind] = &[ChannelKind::Logs, ChannelKind::Control];
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TemplateError {
    #[error("a channel name template must not be empty")]
    Empty,
    #[error("unknown placeholder `{{{0}}}`; available: slug, name, kind")]
    UnknownPlaceholder(String),
    #[error("unclosed `{{` in the template")]
    Unclosed,
    #[error("template is {length} characters before substitution; keep it under {max}")]
    TooLong { length: usize, max: usize },
}

/// A user-supplied pattern for naming a channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NameTemplate(String);

impl NameTemplate {
    /// Validate a template. Placeholders are checked now so a typo is reported
    /// while the user is editing settings, not when a project is created.
    pub fn parse(template: &str) -> Result<Self, TemplateError> {
        let trimmed = template.trim();
        if trimmed.is_empty() {
            return Err(TemplateError::Empty);
        }
        // The template itself is capped well below the channel limit; the
        // substituted result is truncated separately.
        if trimmed.len() > MAX_CHANNEL_NAME_LENGTH {
            return Err(TemplateError::TooLong {
                length: trimmed.len(),
                max: MAX_CHANNEL_NAME_LENGTH,
            });
        }

        let mut rest = trimmed;
        while let Some(open) = rest.find('{') {
            let after = rest.get(open + 1..).ok_or(TemplateError::Unclosed)?;
            let close = after.find('}').ok_or(TemplateError::Unclosed)?;
            let placeholder = after.get(..close).ok_or(TemplateError::Unclosed)?;
            if !matches!(placeholder, "slug" | "name" | "kind") {
                return Err(TemplateError::UnknownPlaceholder(placeholder.to_string()));
            }
            rest = after.get(close + 1..).ok_or(TemplateError::Unclosed)?;
        }

        Ok(Self(trimmed.to_string()))
    }

    pub fn default_for(kind: ChannelKind) -> Self {
        Self(kind.default_template().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Fill the template in and sanitise the result into something Discord will
    /// accept. Always produces a usable name.
    pub fn render(&self, slug: &str, display_name: &str, kind: ChannelKind) -> String {
        let filled = self
            .0
            .replace("{slug}", slug)
            .replace("{name}", display_name)
            .replace("{kind}", kind.as_str());
        sanitise_channel_name(&filled)
    }
}

/// Reduce arbitrary text to a name Discord will accept for a text channel.
///
/// Discord lowercases text channel names and converts spaces itself, but doing
/// it here means the name stored alongside the channel matches the name the
/// user will actually see, so the settings screen does not appear to have
/// silently ignored what was typed.
///
/// Truncation is by character, not by byte, so a multi-byte name cannot be cut
/// mid-character into invalid UTF-8.
pub fn sanitise_channel_name(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_was_dash = false;

    for character in raw.chars() {
        let mapped = if character.is_ascii_alphanumeric() {
            character.to_ascii_lowercase()
        } else if character == '_' {
            '_'
        } else {
            '-'
        };

        if mapped == '-' {
            // Collapse runs, and never start with one.
            if last_was_dash || out.is_empty() {
                continue;
            }
            last_was_dash = true;
        } else {
            last_was_dash = false;
        }

        if out.chars().count() >= MAX_CHANNEL_NAME_LENGTH {
            break;
        }
        out.push(mapped);
    }

    while out.ends_with('-') {
        out.pop();
    }

    if out.is_empty() {
        FALLBACK_NAME.to_string()
    } else {
        out
    }
}

/// The two channels belonging to one project, as Discord created them.
///
/// The ids are the identity. The names are kept only so the interface can show
/// what the channels are called without a round trip to Discord.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectChannels {
    pub guild_id: Snowflake,
    pub logs_channel_id: Snowflake,
    pub control_channel_id: Snowflake,
    pub logs_channel_name: String,
    pub control_channel_name: String,
    /// The panel message, so it can be edited in place rather than reposted on
    /// every status change.
    pub control_message_id: Option<Snowflake>,
}

impl ProjectChannels {
    pub fn channel_for(&self, kind: ChannelKind) -> Snowflake {
        match kind {
            ChannelKind::Logs => self.logs_channel_id,
            ChannelKind::Control => self.control_channel_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_templates_produce_the_expected_names() {
        assert_eq!(
            NameTemplate::default_for(ChannelKind::Logs).render(
                "my-bot",
                "My Bot",
                ChannelKind::Logs
            ),
            "my-bot-logs"
        );
        assert_eq!(
            NameTemplate::default_for(ChannelKind::Control).render(
                "my-bot",
                "My Bot",
                ChannelKind::Control
            ),
            "my-bot-control"
        );
    }

    #[test]
    fn a_display_name_with_spaces_and_case_becomes_a_valid_channel_name() {
        let template = NameTemplate::parse("{name} {kind}").expect("valid");
        assert_eq!(
            template.render("my-bot", "My Cool Bot", ChannelKind::Logs),
            "my-cool-bot-logs"
        );
    }

    #[test]
    fn a_hostile_display_name_cannot_produce_a_strange_channel_name() {
        // The name is user input. None of this should survive.
        let template = NameTemplate::parse("{name}").expect("valid");
        for hostile in [
            "../../etc/passwd",
            "@everyone",
            "#general",
            "<@&123456789>",
            "name\nwith\nnewlines",
            "drop table channels;--",
        ] {
            let rendered = template.render("slug", hostile, ChannelKind::Logs);
            assert!(
                rendered
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'),
                "{hostile:?} rendered as {rendered:?}"
            );
            assert!(!rendered.is_empty());
        }
    }

    #[test]
    fn a_mention_cannot_survive_into_a_channel_name() {
        // A channel literally named `@everyone` would not ping anyone, but the
        // characters that make mentions work have no business in a name.
        let template = NameTemplate::parse("{name}").expect("valid");
        let rendered = template.render("slug", "@everyone @here", ChannelKind::Logs);
        assert!(!rendered.contains('@'));
    }

    #[test]
    fn runs_of_separators_collapse() {
        assert_eq!(sanitise_channel_name("a   b---c"), "a-b-c");
    }

    #[test]
    fn a_name_never_starts_or_ends_with_a_separator() {
        assert_eq!(sanitise_channel_name("---hello---"), "hello");
        assert_eq!(sanitise_channel_name("   spaced   "), "spaced");
    }

    #[test]
    fn a_name_that_sanitises_to_nothing_falls_back_rather_than_failing() {
        // Creating a project must not fail because its name was all emoji.
        for empty in ["", "   ", "🎉🎉🎉", "!!!", "---"] {
            let rendered = sanitise_channel_name(empty);
            assert_eq!(rendered, FALLBACK_NAME, "{empty:?} should fall back");
        }
    }

    #[test]
    fn an_over_long_name_is_truncated_to_discords_limit() {
        let long = "a".repeat(500);
        let rendered = sanitise_channel_name(&long);
        assert_eq!(rendered.chars().count(), MAX_CHANNEL_NAME_LENGTH);
    }

    #[test]
    fn truncation_counts_characters_not_bytes() {
        // Cutting at a byte offset would split a multi-byte character and
        // produce invalid UTF-8 — or, in Rust, panic.
        let long = "é".repeat(500);
        let rendered = sanitise_channel_name(&long);
        assert!(rendered.chars().count() <= MAX_CHANNEL_NAME_LENGTH);
        assert!(rendered.is_char_boundary(rendered.len()));
    }

    #[test]
    fn underscores_are_kept_because_discord_allows_them() {
        assert_eq!(sanitise_channel_name("my_bot_logs"), "my_bot_logs");
    }

    #[test]
    fn an_unknown_placeholder_is_reported_while_editing_settings() {
        let error = NameTemplate::parse("{project}-logs").expect_err("unknown placeholder");
        assert_eq!(
            error,
            TemplateError::UnknownPlaceholder("project".to_string())
        );
    }

    #[test]
    fn an_unclosed_placeholder_is_refused() {
        assert_eq!(
            NameTemplate::parse("{slug-logs"),
            Err(TemplateError::Unclosed)
        );
    }

    #[test]
    fn an_empty_template_is_refused() {
        for empty in ["", "   "] {
            assert_eq!(NameTemplate::parse(empty), Err(TemplateError::Empty));
        }
    }

    #[test]
    fn every_placeholder_the_error_message_advertises_actually_works() {
        // A help string that lists a placeholder the parser rejects is worse
        // than no help string.
        for placeholder in ["slug", "name", "kind"] {
            let template = format!("prefix-{{{placeholder}}}");
            assert!(
                NameTemplate::parse(&template).is_ok(),
                "{placeholder} should be accepted"
            );
        }
    }

    #[test]
    fn every_channel_kind_has_a_valid_default_template() {
        for kind in ChannelKind::ALL {
            let template = NameTemplate::parse(kind.default_template())
                .unwrap_or_else(|error| panic!("{kind:?} default is invalid: {error}"));
            let rendered = template.render("slug", "Name", *kind);
            assert!(!rendered.is_empty());
            assert!(rendered.chars().count() <= MAX_CHANNEL_NAME_LENGTH);
        }
    }

    #[test]
    fn a_template_can_be_rendered_for_either_channel_without_colliding() {
        // Two channels with the same name is legal in Discord and thoroughly
        // confusing, so the defaults must differ.
        let logs =
            NameTemplate::default_for(ChannelKind::Logs).render("bot", "Bot", ChannelKind::Logs);
        let control = NameTemplate::default_for(ChannelKind::Control).render(
            "bot",
            "Bot",
            ChannelKind::Control,
        );
        assert_ne!(logs, control);
    }
}
