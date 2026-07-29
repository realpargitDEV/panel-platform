//! The audit log.
//!
//! Two rules shape this module.
//!
//! First, an audit write must not be able to fail the operation it is recording.
//! A failed insert is logged and swallowed by [`record`] — an agent that refuses
//! to stop a container because it could not write a log line is worse than one
//! with a gap in its log, and the gap is visible.
//!
//! Second, no secret ever reaches a row. `metadata` is a JSON string assembled
//! by the caller, and [`sanitise_metadata`] is the single place that decides
//! what may appear in it: keys are kept, values of anything secret-shaped are
//! replaced. It is applied here rather than trusted to every call site.

use project_host_api_types::AuditId;
use sqlx::Row;

use crate::error::Result;
use crate::time;
use crate::Database;

/// The outcome being recorded. A denied action is not the same as a failed one:
/// one is the system working, the other is the system breaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditResult {
    Success,
    Failure,
    Denied,
}

impl AuditResult {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "SUCCESS",
            Self::Failure => "FAILURE",
            Self::Denied => "DENIED",
        }
    }
}

/// One event, before it is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    pub user_id: Option<String>,
    pub client_id: Option<String>,
    pub client_label: Option<String>,
    pub source_addr: Option<String>,
    /// A stable dotted name — `project.start`, `env.update`, `auth.login`.
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    /// A human-readable name for the target, so a deleted project is still
    /// identifiable in the log after its row is gone.
    pub target_label: Option<String>,
    pub result: AuditResult,
    pub error_code: Option<String>,
    pub request_id: Option<String>,
    pub operation_id: Option<String>,
    /// Key/value pairs. Values are sanitised before storage.
    pub metadata: Vec<(String, String)>,
}

impl AuditEvent {
    pub fn new(action: &str, result: AuditResult) -> Self {
        Self {
            user_id: None,
            client_id: None,
            client_label: None,
            source_addr: None,
            action: action.to_string(),
            target_type: None,
            target_id: None,
            target_label: None,
            result,
            error_code: None,
            request_id: None,
            operation_id: None,
            metadata: Vec::new(),
        }
    }

    pub fn by(mut self, user_id: &str) -> Self {
        self.user_id = Some(user_id.to_string());
        self
    }

    pub fn from_client(mut self, label: &str, source_addr: Option<&str>) -> Self {
        self.client_label = Some(label.to_string());
        self.source_addr = source_addr.map(str::to_string);
        self
    }

    pub fn about(mut self, target_type: &str, target_id: &str, label: Option<&str>) -> Self {
        self.target_type = Some(target_type.to_string());
        self.target_id = Some(target_id.to_string());
        self.target_label = label.map(str::to_string);
        self
    }

    pub fn with_request(mut self, request_id: &str) -> Self {
        self.request_id = Some(request_id.to_string());
        self
    }

    pub fn with_operation(mut self, operation_id: &str) -> Self {
        self.operation_id = Some(operation_id.to_string());
        self
    }

    pub fn failed_with(mut self, error_code: &str) -> Self {
        self.error_code = Some(error_code.to_string());
        self
    }

    pub fn detail(mut self, key: &str, value: impl std::fmt::Display) -> Self {
        self.metadata.push((key.to_string(), value.to_string()));
        self
    }
}

/// Metadata keys whose values are replaced rather than stored.
///
/// The match is on the *key*, because the value is the thing we must not look
/// at. Matching on value shape would mean deciding a token is not a token.
///
/// The list errs towards redacting: `key` is on it, so a call site that wants to
/// record *which* environment variable changed uses `variable` for the name and
/// leaves `value` to be redacted. Naming the field precisely is the call site's
/// job; guessing on its behalf is how a secret ends up in the log.
const SENSITIVE_KEYS: &[&str] = &[
    "value",
    "secret",
    "token",
    "password",
    "passwd",
    "key",
    "credential",
    "authorization",
    "cookie",
    "session",
    "hash",
    "recovery_code",
    "private",
];

const REDACTED: &str = "[redacted]";
const MAX_VALUE_CHARS: usize = 512;

fn is_sensitive(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    SENSITIVE_KEYS
        .iter()
        .any(|marker| lower == *marker || lower.contains(marker))
}

/// Turn metadata pairs into a JSON object with sensitive values removed.
///
/// Serialised by hand rather than with `serde_json` so this crate does not gain
/// a JSON dependency for one field; the escaping below covers everything JSON
/// requires in a string.
pub fn sanitise_metadata(pairs: &[(String, String)]) -> Option<String> {
    if pairs.is_empty() {
        return None;
    }

    let mut out = String::from("{");
    for (index, (key, value)) in pairs.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let shown = if is_sensitive(key) {
            REDACTED.to_string()
        } else if value.chars().count() > MAX_VALUE_CHARS {
            let truncated: String = value.chars().take(MAX_VALUE_CHARS).collect();
            format!("{truncated}…")
        } else {
            value.clone()
        };
        out.push_str(&json_string(key));
        out.push(':');
        out.push_str(&json_string(&shown));
    }
    out.push('}');
    Some(out)
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Write an event, returning its identifier.
///
/// Prefer [`record`] in request handlers: this variant propagates a storage
/// failure, which is only what you want in a test or a migration.
pub async fn write(database: &Database, event: &AuditEvent) -> Result<String> {
    let id = AuditId::generate().to_string();
    sqlx::query(
        "INSERT INTO audit_logs (id, occurred_at, user_id, client_id, client_label,
                                 source_addr, action, target_type, target_id, target_label,
                                 result, error_code, request_id, operation_id, metadata)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(time::now())
    .bind(&event.user_id)
    .bind(&event.client_id)
    .bind(&event.client_label)
    .bind(&event.source_addr)
    .bind(&event.action)
    .bind(&event.target_type)
    .bind(&event.target_id)
    .bind(&event.target_label)
    .bind(event.result.as_str())
    .bind(&event.error_code)
    .bind(&event.request_id)
    .bind(&event.operation_id)
    .bind(sanitise_metadata(&event.metadata))
    .execute(database.pool())
    .await?;
    Ok(id)
}

/// Write an event, never failing the caller.
///
/// The failure is reported to the structured log, which is where an operator
/// looking for a missing audit entry would go next.
pub async fn record(database: &Database, event: AuditEvent) {
    if let Err(error) = write(database, &event).await {
        tracing::error!(
            action = %event.action,
            error = %error,
            "failed to write an audit entry"
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRecord {
    pub id: String,
    pub occurred_at: String,
    pub user_id: Option<String>,
    pub client_label: Option<String>,
    pub source_addr: Option<String>,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub target_label: Option<String>,
    pub result: String,
    pub error_code: Option<String>,
    pub request_id: Option<String>,
    pub operation_id: Option<String>,
    pub metadata: Option<String>,
}

/// A filtered page of the log, newest first.
///
/// Paginated by `(occurred_at, id)` rather than an offset: entries are appended
/// constantly, and an offset page would shift under the reader.
pub async fn list(
    database: &Database,
    action_prefix: Option<&str>,
    target_id: Option<&str>,
    before: Option<(&str, &str)>,
    limit: u32,
) -> Result<Vec<AuditRecord>> {
    let (before_time, before_id) = match before {
        Some((time, id)) => (Some(time), Some(id)),
        None => (None, None),
    };

    let rows = sqlx::query(
        "SELECT id, occurred_at, user_id, client_label, source_addr, action,
                target_type, target_id, target_label, result, error_code,
                request_id, operation_id, metadata
         FROM audit_logs
         WHERE (? IS NULL OR action LIKE ? || '%')
           AND (? IS NULL OR target_id = ?)
           AND (? IS NULL OR (occurred_at, id) < (?, ?))
         ORDER BY occurred_at DESC, id DESC
         LIMIT ?",
    )
    .bind(action_prefix)
    .bind(action_prefix)
    .bind(target_id)
    .bind(target_id)
    .bind(before_time)
    .bind(before_time)
    .bind(before_id)
    .bind(i64::from(limit.clamp(1, 500)))
    .fetch_all(database.pool())
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| AuditRecord {
            id: row.get("id"),
            occurred_at: row.get("occurred_at"),
            user_id: row.get("user_id"),
            client_label: row.get("client_label"),
            source_addr: row.get("source_addr"),
            action: row.get("action"),
            target_type: row.get("target_type"),
            target_id: row.get("target_id"),
            target_label: row.get("target_label"),
            result: row.get("result"),
            error_code: row.get("error_code"),
            request_id: row.get("request_id"),
            operation_id: row.get("operation_id"),
            metadata: row.get("metadata"),
        })
        .collect())
}

/// Trim the log to a maximum number of entries.
///
/// Count-based rather than age-based: a quiet installation should keep its
/// history, and a busy one should not fill the disk.
pub async fn prune_to(database: &Database, keep: u32) -> Result<u64> {
    let result = sqlx::query(
        "DELETE FROM audit_logs WHERE id NOT IN (
            SELECT id FROM audit_logs ORDER BY occurred_at DESC, id DESC LIMIT ?
         )",
    )
    .bind(i64::from(keep))
    .execute(database.pool())
    .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_keys_are_redacted_whatever_their_case_or_prefix() {
        let metadata = sanitise_metadata(&[
            ("key".to_string(), "should-not-survive".to_string()),
            ("VALUE".to_string(), "hunter2".to_string()),
            ("new_password".to_string(), "hunter2".to_string()),
            ("session_token".to_string(), "abc".to_string()),
            // The name a call site uses when it wants the variable's name kept.
            ("variable".to_string(), "DISCORD_TOKEN".to_string()),
            ("count".to_string(), "3".to_string()),
        ])
        .expect("metadata");

        assert!(!metadata.contains("hunter2"), "{metadata}");
        assert!(!metadata.contains("should-not-survive"), "{metadata}");
        assert!(!metadata.contains("\":\"abc\""), "{metadata}");
        assert!(metadata.contains("\"count\":\"3\""), "{metadata}");
        // Knowing *which* variable changed is the point of the entry.
        assert!(metadata.contains("DISCORD_TOKEN"), "{metadata}");
    }

    #[test]
    fn empty_metadata_is_absent_rather_than_an_empty_object() {
        assert_eq!(sanitise_metadata(&[]), None);
    }

    #[test]
    fn metadata_is_valid_json_even_with_hostile_content() {
        let metadata = sanitise_metadata(&[(
            "detail".to_string(),
            "he said \"hi\"\n\tand \\ left\u{1}".to_string(),
        )])
        .expect("metadata");

        assert_eq!(
            metadata,
            "{\"detail\":\"he said \\\"hi\\\"\\n\\tand \\\\ left\\u0001\"}"
        );
    }

    #[test]
    fn a_very_long_value_is_truncated() {
        let long = "x".repeat(MAX_VALUE_CHARS * 2);
        let metadata = sanitise_metadata(&[("detail".to_string(), long)]).expect("metadata");
        assert!(metadata.chars().count() < MAX_VALUE_CHARS + 40);
        assert!(metadata.contains('…'));
    }

    #[test]
    fn the_builder_produces_the_event_it_describes() {
        let event = AuditEvent::new("project.start", AuditResult::Success)
            .by("usr_1")
            .about("project", "prj_1", Some("My Bot"))
            .with_request("req_1")
            .detail("restart_count", 2);

        assert_eq!(event.action, "project.start");
        assert_eq!(event.result, AuditResult::Success);
        assert_eq!(event.target_label.as_deref(), Some("My Bot"));
        assert_eq!(
            event.metadata,
            vec![("restart_count".to_string(), "2".to_string())]
        );
    }
}
