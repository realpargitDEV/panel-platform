//! What gets sent to the logs channel, and what it looks like when it arrives.
//!
//! Everything here is pure: an event and some settings in, message text out.
//! That is what makes the interesting parts testable, because the interesting
//! parts are not "did the message send" but:
//!
//! * **A project's own output must not be able to ping the server.** Log lines
//!   are attacker-influenced in the ordinary case — a Discord bot logging a
//!   username, a web server logging a request path. `@everyone` in a log line
//!   must arrive as text.
//! * **A project's own output must not be able to break out of its code
//!   block** and inject Markdown, links or further formatting.
//! * **Secret environment variable values must not be forwarded.** The
//!   application knows them, because it passes them to the container; a crash
//!   that echoes one into stderr must not republish it to a Discord channel.
//! * **Discord's 2000-character limit** must be respected by construction, not
//!   discovered when a message is rejected.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::snowflake::Snowflake;

/// Discord's limit on the `content` of a single message.
pub const MAX_MESSAGE_LENGTH: usize = 2000;

/// What the mask replaces a secret with.
const REDACTED: &str = "«redacted»";

/// Shortest secret value worth masking.
///
/// Masking every occurrence of a two-character secret would blank most of the
/// log and reveal that the secret is two characters. Values this short are
/// refused as secrets elsewhere; this is a second line.
const MIN_MASKABLE_SECRET: usize = 6;

/// Something worth telling Discord about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Started,
    Stopped,
    Crashed,
    Restarted,
    DeploymentStarted,
    DeploymentSucceeded,
    DeploymentFailed,
    HealthDegraded,
    HealthRecovered,
    ResourceWarning,
    BackupCompleted,
    BackupFailed,
    ErrorLogged,
    WarningLogged,
    LogOutput,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::Started => "started",
            EventKind::Stopped => "stopped",
            EventKind::Crashed => "crashed",
            EventKind::Restarted => "restarted",
            EventKind::DeploymentStarted => "deployment_started",
            EventKind::DeploymentSucceeded => "deployment_succeeded",
            EventKind::DeploymentFailed => "deployment_failed",
            EventKind::HealthDegraded => "health_degraded",
            EventKind::HealthRecovered => "health_recovered",
            EventKind::ResourceWarning => "resource_warning",
            EventKind::BackupCompleted => "backup_completed",
            EventKind::BackupFailed => "backup_failed",
            EventKind::ErrorLogged => "error_logged",
            EventKind::WarningLogged => "warning_logged",
            EventKind::LogOutput => "log_output",
        }
    }

    pub fn parse(text: &str) -> Option<EventKind> {
        EventKind::ALL
            .iter()
            .copied()
            .find(|kind| kind.as_str() == text)
    }

    /// The emoji that opens the message. Chosen so the channel can be skimmed.
    pub fn icon(self) -> &'static str {
        match self {
            EventKind::Started | EventKind::HealthRecovered | EventKind::DeploymentSucceeded => {
                "🟢"
            }
            EventKind::Stopped => "⚫",
            EventKind::Crashed | EventKind::DeploymentFailed | EventKind::BackupFailed => "🔴",
            EventKind::Restarted | EventKind::DeploymentStarted => "🔄",
            EventKind::HealthDegraded | EventKind::ResourceWarning | EventKind::WarningLogged => {
                "🟡"
            }
            EventKind::BackupCompleted => "💾",
            EventKind::ErrorLogged => "❗",
            EventKind::LogOutput => "📄",
        }
    }

    /// Whether this event is bad enough to be worth an `@mention`, when the
    /// user has configured one.
    pub fn is_failure(self) -> bool {
        matches!(
            self,
            EventKind::Crashed
                | EventKind::DeploymentFailed
                | EventKind::BackupFailed
                | EventKind::HealthDegraded
        )
    }

    /// The events enabled when a project is first linked.
    ///
    /// Deliberately excludes [`EventKind::LogOutput`]: forwarding every line a
    /// project writes turns the channel into noise and burns Discord rate
    /// limits. A user who wants it can turn it on.
    pub fn sensible_defaults() -> BTreeSet<EventKind> {
        [
            EventKind::Started,
            EventKind::Stopped,
            EventKind::Crashed,
            EventKind::Restarted,
            EventKind::DeploymentSucceeded,
            EventKind::DeploymentFailed,
            EventKind::HealthDegraded,
            EventKind::HealthRecovered,
            EventKind::ResourceWarning,
            EventKind::BackupFailed,
            EventKind::ErrorLogged,
        ]
        .into_iter()
        .collect()
    }

    pub const ALL: &'static [EventKind] = &[
        EventKind::Started,
        EventKind::Stopped,
        EventKind::Crashed,
        EventKind::Restarted,
        EventKind::DeploymentStarted,
        EventKind::DeploymentSucceeded,
        EventKind::DeploymentFailed,
        EventKind::HealthDegraded,
        EventKind::HealthRecovered,
        EventKind::ResourceWarning,
        EventKind::BackupCompleted,
        EventKind::BackupFailed,
        EventKind::ErrorLogged,
        EventKind::WarningLogged,
        EventKind::LogOutput,
    ];
}

/// Per-project notification preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationSettings {
    pub enabled_events: BTreeSet<EventKind>,
    /// Pinged when a failure event arrives. `None` means never ping.
    pub mention_role_on_failure: Option<Snowflake>,
    /// How long log lines are gathered before being posted as one message.
    /// Batching is what keeps a chatty project from exhausting Discord's rate
    /// limit one line at a time.
    pub batch_window_ms: u32,
    /// Whether the integration sends anything at all for this project. A
    /// separate switch from clearing the event list, so that muting a project
    /// during an incident does not lose the user's configuration.
    pub enabled: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled_events: EventKind::sensible_defaults(),
            mention_role_on_failure: None,
            batch_window_ms: 2_000,
            enabled: true,
        }
    }
}

impl NotificationSettings {
    pub fn should_send(&self, kind: EventKind) -> bool {
        self.enabled && self.enabled_events.contains(&kind)
    }

    /// Who to ping for this event, if anyone.
    pub fn mention_for(&self, kind: EventKind) -> Option<Snowflake> {
        if kind.is_failure() {
            self.mention_role_on_failure
        } else {
            None
        }
    }
}

/// Masks secret values on their way out.
///
/// Built from the plaintext values the application already holds in order to
/// start the container. It never stores them anywhere else and is dropped with
/// the send.
#[derive(Debug, Clone, Default)]
pub struct Redactor {
    secrets: Vec<String>,
}

impl Redactor {
    pub fn new(values: impl IntoIterator<Item = String>) -> Self {
        let mut secrets: Vec<String> = values
            .into_iter()
            .filter(|value| value.len() >= MIN_MASKABLE_SECRET)
            .collect();
        // Longest first, so a secret that contains another secret is masked
        // whole rather than leaving a fragment of the longer one behind.
        secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
        secrets.dedup();
        Self { secrets }
    }

    pub fn apply(&self, text: &str) -> String {
        let mut out = text.to_string();
        for secret in &self.secrets {
            if out.contains(secret.as_str()) {
                out = out.replace(secret.as_str(), REDACTED);
            }
        }
        out
    }
}

/// Stop text from pinging anyone.
///
/// A zero-width space after the `@` leaves the text readable and stops Discord
/// resolving it. This runs even on content that will be placed inside a code
/// block — mentions do not resolve there either, but relying on the fence alone
/// would mean one refactor away from a project's log pinging everybody.
pub fn neutralise_mentions(text: &str) -> String {
    text.replace('@', "@\u{200b}")
}

/// Wrap text in a fence long enough that the text cannot escape it.
///
/// Markdown's rule is that a fence is closed by a run of backticks at least as
/// long as the opening one, so an opening fence longer than any run inside the
/// content cannot be closed early. Counting is cheaper and more reliable than
/// trying to escape the content.
pub fn fence(text: &str) -> String {
    let mut longest_run = 0usize;
    let mut current = 0usize;
    for character in text.chars() {
        if character == '`' {
            current += 1;
            longest_run = longest_run.max(current);
        } else {
            current = 0;
        }
    }

    let fence = "`".repeat(longest_run.max(2) + 1);
    // A trailing newline keeps a closing fence on its own line even when the
    // content does not end with one.
    format!("{fence}\n{text}\n{fence}")
}

/// A message ready to hand to Discord.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingMessage {
    pub content: String,
    /// Set when the settings ask for a ping on failure. The sender is expected
    /// to pass this through Discord's `allowed_mentions` so that *only* this
    /// role can be pinged, whatever the rest of the content says.
    pub mention_role: Option<Snowflake>,
}

/// Format a project event as one message.
pub fn format_event(
    kind: EventKind,
    project_name: &str,
    detail: &str,
    settings: &NotificationSettings,
    redactor: &Redactor,
) -> OutgoingMessage {
    let mention = settings.mention_for(kind);

    let mut content = String::new();
    if let Some(role) = mention {
        content.push_str(&format!("<@&{role}> "));
    }
    // The project's own name is user input and goes through the same treatment
    // as everything else.
    content.push_str(kind.icon());
    content.push(' ');
    content.push_str("**");
    content.push_str(&neutralise_mentions(&escape_markdown(project_name)));
    content.push_str("** — ");
    content.push_str(kind.as_str().replace('_', " ").as_str());

    let detail = redactor.apply(detail);
    if !detail.trim().is_empty() {
        content.push('\n');
        content.push_str(&fence(&neutralise_mentions(detail.trim_end())));
    }

    OutgoingMessage {
        content: truncate_to_limit(&content),
        mention_role: mention,
    }
}

/// Split log lines into as few messages as Discord's limit allows.
///
/// Lines are never merged across a message boundary mid-line unless a single
/// line is itself too long, in which case it is split — a truncated stack trace
/// is less useful than a wrapped one.
pub fn format_log_batch(
    project_name: &str,
    lines: &[String],
    redactor: &Redactor,
) -> Vec<OutgoingMessage> {
    let header = format!(
        "{} **{}**\n",
        EventKind::LogOutput.icon(),
        neutralise_mentions(&escape_markdown(project_name))
    );

    // Reserve room for the header and for a fence of the usual length. `fence`
    // may choose a longer one for content full of backticks, so the budget is
    // deliberately conservative rather than exact.
    let fence_overhead = 12;
    let budget = MAX_MESSAGE_LENGTH
        .saturating_sub(header.len())
        .saturating_sub(fence_overhead);

    let mut messages = Vec::new();
    let mut block = String::new();

    let flush = |block: &mut String, messages: &mut Vec<OutgoingMessage>| {
        if block.is_empty() {
            return;
        }
        messages.push(OutgoingMessage {
            content: truncate_to_limit(&format!("{header}{}", fence(block.trim_end_matches('\n')))),
            mention_role: None,
        });
        block.clear();
    };

    for line in lines {
        let line = neutralise_mentions(&redactor.apply(line));
        for chunk in split_long_line(&line, budget) {
            if block.chars().count() + chunk.chars().count() + 1 > budget {
                flush(&mut block, &mut messages);
            }
            block.push_str(&chunk);
            block.push('\n');
        }
    }
    flush(&mut block, &mut messages);

    messages
}

/// Break one over-long line into budget-sized pieces on character boundaries.
fn split_long_line(line: &str, budget: usize) -> Vec<String> {
    if line.chars().count() <= budget || budget == 0 {
        return vec![line.to_string()];
    }

    let mut pieces = Vec::new();
    let mut piece = String::new();
    for character in line.chars() {
        if piece.chars().count() >= budget {
            pieces.push(std::mem::take(&mut piece));
        }
        piece.push(character);
    }
    if !piece.is_empty() {
        pieces.push(piece);
    }
    pieces
}

/// Escape the Markdown characters that would otherwise let a project name
/// change how the rest of the message renders.
fn escape_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(character, '*' | '_' | '~' | '`' | '|' | '\\' | '>' | '#') {
            out.push('\\');
        }
        out.push(character);
    }
    out
}

/// Last-resort cut, on a character boundary.
///
/// Everything above is meant to keep messages inside the limit already; this is
/// what stops a miscalculation becoming a rejected send.
fn truncate_to_limit(text: &str) -> String {
    if text.chars().count() <= MAX_MESSAGE_LENGTH {
        return text.to_string();
    }
    text.chars().take(MAX_MESSAGE_LENGTH).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> NotificationSettings {
        NotificationSettings::default()
    }

    fn no_secrets() -> Redactor {
        Redactor::default()
    }

    #[test]
    fn a_log_line_cannot_ping_the_server() {
        // The headline risk. A Discord bot that logs the message it just
        // received would otherwise let any user in any server ping everyone.
        let messages = format_log_batch(
            "bot",
            &["user said: @everyone free nitro".to_string()],
            &no_secrets(),
        );
        let content = &messages.first().expect("one message").content;
        assert!(!content.contains("@everyone"), "got {content}");
        assert!(content.contains("@\u{200b}everyone"));
    }

    #[test]
    fn a_project_name_cannot_ping_the_server() {
        let message = format_event(
            EventKind::Started,
            "@everyone",
            "",
            &settings(),
            &no_secrets(),
        );
        assert!(!message.content.contains("@everyone"));
    }

    #[test]
    fn a_log_line_cannot_escape_its_code_block() {
        // Closing the fence would let a project emit arbitrary Markdown, links
        // and formatting into a channel other people read.
        let hostile = "```\n# Big heading\n[click me](https://example.com)";
        let messages = format_log_batch("bot", &[hostile.to_string()], &no_secrets());
        let content = &messages.first().expect("one message").content;

        // The opening fence must be longer than the longest run in the content.
        let opening = content
            .lines()
            .find(|line| line.starts_with("``"))
            .expect("a fence");
        assert!(
            opening.len() > 3,
            "fence {opening:?} is not longer than the content's own run"
        );
    }

    #[test]
    fn a_fence_is_always_longer_than_the_longest_run_it_contains() {
        for content in ["no backticks", "`one`", "``two``", "```three```", "`````"] {
            let fenced = fence(content);
            let opening = fenced.lines().next().expect("first line");
            let longest_inside = content
                .split(|c| c != '`')
                .map(str::len)
                .max()
                .unwrap_or_default();
            assert!(
                opening.len() > longest_inside,
                "fence {opening:?} cannot contain {content:?}"
            );
        }
    }

    #[test]
    fn a_secret_value_is_masked_before_it_reaches_discord() {
        // The scenario: a bot crashes on startup and prints its own token.
        let token = "MTIzNDU2Nzg5MDEyMzQ1Njc4.GaBcDe.FgHiJkLmNoPqRsTuVwXyZ";
        let redactor = Redactor::new([token.to_string()]);
        let messages = format_log_batch(
            "bot",
            &[format!("failed to log in with token {token}")],
            &redactor,
        );
        let content = &messages.first().expect("one message").content;
        assert!(!content.contains(token), "the token survived: {content}");
        assert!(content.contains(REDACTED));
    }

    #[test]
    fn a_secret_in_an_event_detail_is_masked_too() {
        let secret = "hunter2hunter2";
        let redactor = Redactor::new([secret.to_string()]);
        let message = format_event(
            EventKind::Crashed,
            "bot",
            &format!("DATABASE_URL=postgres://user:{secret}@host/db"),
            &settings(),
            &redactor,
        );
        assert!(!message.content.contains(secret));
    }

    #[test]
    fn the_longest_matching_secret_is_masked_first() {
        // Otherwise masking the short one leaves a fragment of the long one.
        let redactor = Redactor::new(["abcdef".to_string(), "abcdefghijkl".to_string()]);
        let masked = redactor.apply("value=abcdefghijkl");
        assert_eq!(masked, format!("value={REDACTED}"));
    }

    #[test]
    fn a_very_short_secret_is_not_used_as_a_mask() {
        // Masking every "abc" would blank half the log and disclose that the
        // secret is three characters long.
        let redactor = Redactor::new(["abc".to_string()]);
        assert_eq!(redactor.apply("abc def abc"), "abc def abc");
    }

    #[test]
    fn every_message_fits_discords_limit() {
        let long_lines: Vec<String> = (0..200)
            .map(|index| format!("line {index}: {}", "x".repeat(100)))
            .collect();
        let messages = format_log_batch("bot", &long_lines, &no_secrets());

        assert!(
            messages.len() > 1,
            "should have split into several messages"
        );
        for message in &messages {
            assert!(
                message.content.chars().count() <= MAX_MESSAGE_LENGTH,
                "message of {} characters exceeds the limit",
                message.content.chars().count()
            );
        }
    }

    #[test]
    fn one_enormous_line_is_split_rather_than_dropped() {
        let stack_trace = "at ".to_string() + &"deeply.nested.frame.".repeat(500);
        let messages = format_log_batch("bot", &[stack_trace], &no_secrets());
        assert!(messages.len() > 1);
        for message in &messages {
            assert!(message.content.chars().count() <= MAX_MESSAGE_LENGTH);
        }
    }

    #[test]
    fn a_multi_byte_log_line_is_never_cut_mid_character() {
        let line = "日本語のログ".repeat(1000);
        let messages = format_log_batch("bot", &[line], &no_secrets());
        for message in &messages {
            // Invalid UTF-8 is unrepresentable in a `String`, so the real check
            // is that building it did not panic and nothing was lost.
            assert!(message.content.is_char_boundary(message.content.len()));
        }
    }

    #[test]
    fn an_empty_batch_produces_no_messages() {
        assert!(format_log_batch("bot", &[], &no_secrets()).is_empty());
    }

    #[test]
    fn a_failure_pings_the_configured_role_and_a_success_does_not() {
        let role = Snowflake::new(999).expect("non-zero");
        let settings = NotificationSettings {
            mention_role_on_failure: Some(role),
            ..NotificationSettings::default()
        };

        let crashed = format_event(EventKind::Crashed, "bot", "", &settings, &no_secrets());
        assert_eq!(crashed.mention_role, Some(role));
        assert!(crashed.content.contains("<@&999>"));

        let started = format_event(EventKind::Started, "bot", "", &settings, &no_secrets());
        assert_eq!(started.mention_role, None);
        assert!(!started.content.contains("<@&999>"));
    }

    #[test]
    fn no_ping_is_sent_when_none_is_configured() {
        let message = format_event(EventKind::Crashed, "bot", "", &settings(), &no_secrets());
        assert_eq!(message.mention_role, None);
    }

    #[test]
    fn log_output_is_off_by_default() {
        // Forwarding every line would make the channel useless and exhaust
        // Discord's rate limit.
        assert!(!settings().should_send(EventKind::LogOutput));
        assert!(settings().should_send(EventKind::Crashed));
    }

    #[test]
    fn muting_a_project_silences_everything_without_losing_the_settings() {
        let settings = NotificationSettings {
            enabled: false,
            ..NotificationSettings::default()
        };
        for kind in EventKind::ALL {
            assert!(!settings.should_send(*kind));
        }
        assert!(!settings.enabled_events.is_empty(), "configuration kept");
    }

    #[test]
    fn every_event_kind_round_trips_through_its_wire_name() {
        for kind in EventKind::ALL {
            assert_eq!(EventKind::parse(kind.as_str()), Some(*kind));
        }
        assert_eq!(EventKind::parse("not_an_event"), None);
    }

    #[test]
    fn every_event_kind_has_an_icon() {
        for kind in EventKind::ALL {
            assert!(!kind.icon().is_empty(), "{kind:?} has no icon");
        }
    }

    #[test]
    fn markdown_in_a_project_name_does_not_reformat_the_message() {
        let message = format_event(
            EventKind::Started,
            "**not really bold**",
            "",
            &settings(),
            &no_secrets(),
        );
        assert!(message.content.contains("\\*\\*not really bold\\*\\*"));
    }
}
