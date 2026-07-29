//! Storage for the Discord integration.
//!
//! Like [`crate::environment`], this module never sees an encryption key. The
//! bot token arrives as ciphertext and leaves as ciphertext; whoever calls it
//! owns the key. That keeps the one piece of code that can turn a stored blob
//! back into a usable token out of the layer that talks to SQLite.
//!
//! It also does not depend on the `discord` crate. The rows here are plain
//! records — a permission level is a `String`, not a `Permission`. The
//! composition layer assembles them into an access policy, which keeps the
//! storage and the rules independently testable and stops a schema change from
//! rippling into the authorisation logic.

use project_host_api_types::ids::{GrantId, GuildLinkId};
use sqlx::Row;

use crate::error::{DatabaseError, Result};
use crate::time;
use crate::Database;

/// The bot's credentials, encrypted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotCredentials {
    pub application_id: String,
    pub token_cipher: Vec<u8>,
    pub token_nonce: Vec<u8>,
}

/// A linked Discord server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuildRecord {
    pub id: String,
    pub guild_id: String,
    pub guild_name: String,
    pub linked_by_user_id: String,
    pub allow_guild_owner: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// One permission grant, exactly as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantRecord {
    pub id: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub level: String,
}

/// A project's Discord channels and notification preferences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelRecord {
    pub project_id: String,
    pub guild_row_id: String,
    pub logs_channel_id: String,
    pub control_channel_id: String,
    pub logs_channel_name: String,
    pub control_channel_name: String,
    pub control_message_id: Option<String>,
    pub enabled: bool,
    pub mention_role_on_failure: Option<String>,
    pub batch_window_ms: u32,
}

/// What to write when linking a server.
#[derive(Debug, Clone)]
pub struct NewGuildLink {
    pub guild_id: String,
    pub guild_name: String,
    pub linked_by_user_id: String,
    pub allow_guild_owner: bool,
}

/// What to write when a project's channels have been created.
#[derive(Debug, Clone)]
pub struct NewChannels {
    pub project_id: String,
    pub guild_row_id: String,
    pub logs_channel_id: String,
    pub control_channel_id: String,
    pub logs_channel_name: String,
    pub control_channel_name: String,
}

/// Store or replace the bot's credentials.
///
/// The token is already encrypted. There is no column it could go into
/// otherwise — see the migration.
pub async fn save_bot_credentials(db: &Database, credentials: &BotCredentials) -> Result<()> {
    sqlx::query(
        "INSERT INTO discord_bot (id, application_id, token_cipher, token_nonce, updated_at)
         VALUES (1, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
             application_id = excluded.application_id,
             token_cipher   = excluded.token_cipher,
             token_nonce    = excluded.token_nonce,
             updated_at     = excluded.updated_at",
    )
    .bind(&credentials.application_id)
    .bind(&credentials.token_cipher)
    .bind(&credentials.token_nonce)
    .bind(time::now())
    .execute(db.pool())
    .await?;
    Ok(())
}

pub async fn load_bot_credentials(db: &Database) -> Result<Option<BotCredentials>> {
    let row = sqlx::query(
        "SELECT application_id, token_cipher, token_nonce FROM discord_bot WHERE id = 1",
    )
    .fetch_optional(db.pool())
    .await?;

    Ok(row.map(|row| BotCredentials {
        application_id: row.get("application_id"),
        token_cipher: row.get("token_cipher"),
        token_nonce: row.get("token_nonce"),
    }))
}

/// Forget the bot's credentials.
///
/// Deleting the row rather than blanking the columns, so there is no window in
/// which a zero-length ciphertext looks like a valid token.
pub async fn forget_bot_credentials(db: &Database) -> Result<()> {
    sqlx::query("DELETE FROM discord_bot WHERE id = 1")
        .execute(db.pool())
        .await?;
    Ok(())
}

/// Link a Discord server, returning the row id.
///
/// Re-linking a server already known updates its name and linker rather than
/// failing: the common case is the user running the link flow again after
/// renaming the server.
pub async fn link_guild(db: &Database, link: &NewGuildLink) -> Result<String> {
    let id = GuildLinkId::generate().to_string();
    let now = time::now();

    let existing: Option<String> =
        sqlx::query_scalar("SELECT id FROM discord_guilds WHERE guild_id = ?")
            .bind(&link.guild_id)
            .fetch_optional(db.pool())
            .await?;

    if let Some(existing) = existing {
        sqlx::query(
            "UPDATE discord_guilds
                SET guild_name = ?, linked_by_user_id = ?, allow_guild_owner = ?, updated_at = ?
              WHERE id = ?",
        )
        .bind(&link.guild_name)
        .bind(&link.linked_by_user_id)
        .bind(i64::from(link.allow_guild_owner))
        .bind(&now)
        .bind(&existing)
        .execute(db.pool())
        .await?;
        return Ok(existing);
    }

    sqlx::query(
        "INSERT INTO discord_guilds
            (id, guild_id, guild_name, linked_by_user_id, allow_guild_owner,
             created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&link.guild_id)
    .bind(&link.guild_name)
    .bind(&link.linked_by_user_id)
    .bind(i64::from(link.allow_guild_owner))
    .bind(&now)
    .bind(&now)
    .execute(db.pool())
    .await?;

    Ok(id)
}

pub async fn find_guild(db: &Database, guild_id: &str) -> Result<Option<GuildRecord>> {
    let row = sqlx::query(
        "SELECT id, guild_id, guild_name, linked_by_user_id, allow_guild_owner,
                created_at, updated_at
           FROM discord_guilds WHERE guild_id = ?",
    )
    .bind(guild_id)
    .fetch_optional(db.pool())
    .await?;

    Ok(row.map(guild_from_row))
}

pub async fn list_guilds(db: &Database) -> Result<Vec<GuildRecord>> {
    let rows = sqlx::query(
        "SELECT id, guild_id, guild_name, linked_by_user_id, allow_guild_owner,
                created_at, updated_at
           FROM discord_guilds ORDER BY created_at",
    )
    .fetch_all(db.pool())
    .await?;

    Ok(rows.into_iter().map(guild_from_row).collect())
}

fn guild_from_row(row: sqlx::sqlite::SqliteRow) -> GuildRecord {
    GuildRecord {
        id: row.get("id"),
        guild_id: row.get("guild_id"),
        guild_name: row.get("guild_name"),
        linked_by_user_id: row.get("linked_by_user_id"),
        allow_guild_owner: row.get::<i64, _>("allow_guild_owner") != 0,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

/// Unlink a server. Cascades to its grants, blocks, templates and channel rows.
pub async fn unlink_guild(db: &Database, guild_row_id: &str) -> Result<bool> {
    let result = sqlx::query("DELETE FROM discord_guilds WHERE id = ?")
        .bind(guild_row_id)
        .execute(db.pool())
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Add or replace a grant.
///
/// Upserting rather than inserting is what makes "the highest grant wins"
/// meaningful: two rows for the same role would make the result depend on the
/// order rows came back in.
pub async fn upsert_grant(
    db: &Database,
    guild_row_id: &str,
    subject_kind: &str,
    subject_id: &str,
    level: &str,
) -> Result<String> {
    if !matches!(subject_kind, "role" | "user") {
        return Err(DatabaseError::Invalid(format!(
            "unknown grant subject kind `{subject_kind}`"
        )));
    }
    if !matches!(level, "view" | "operate" | "administer") {
        return Err(DatabaseError::Invalid(format!(
            "unknown permission level `{level}`"
        )));
    }

    let id = GrantId::generate().to_string();
    sqlx::query(
        "INSERT INTO discord_grants
            (id, guild_row_id, subject_kind, subject_id, level, created_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(guild_row_id, subject_kind, subject_id)
         DO UPDATE SET level = excluded.level",
    )
    .bind(&id)
    .bind(guild_row_id)
    .bind(subject_kind)
    .bind(subject_id)
    .bind(level)
    .bind(time::now())
    .execute(db.pool())
    .await?;

    let stored: String = sqlx::query_scalar(
        "SELECT id FROM discord_grants
          WHERE guild_row_id = ? AND subject_kind = ? AND subject_id = ?",
    )
    .bind(guild_row_id)
    .bind(subject_kind)
    .bind(subject_id)
    .fetch_one(db.pool())
    .await?;

    Ok(stored)
}

pub async fn list_grants(db: &Database, guild_row_id: &str) -> Result<Vec<GrantRecord>> {
    let rows = sqlx::query(
        "SELECT id, subject_kind, subject_id, level
           FROM discord_grants WHERE guild_row_id = ? ORDER BY created_at",
    )
    .bind(guild_row_id)
    .fetch_all(db.pool())
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| GrantRecord {
            id: row.get("id"),
            subject_kind: row.get("subject_kind"),
            subject_id: row.get("subject_id"),
            level: row.get("level"),
        })
        .collect())
}

pub async fn remove_grant(db: &Database, grant_id: &str) -> Result<bool> {
    let result = sqlx::query("DELETE FROM discord_grants WHERE id = ?")
        .bind(grant_id)
        .execute(db.pool())
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn block_user(
    db: &Database,
    guild_row_id: &str,
    user_id: &str,
    reason: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO discord_blocked_users (guild_row_id, user_id, reason, created_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(guild_row_id, user_id) DO UPDATE SET reason = excluded.reason",
    )
    .bind(guild_row_id)
    .bind(user_id)
    .bind(reason)
    .bind(time::now())
    .execute(db.pool())
    .await?;
    Ok(())
}

pub async fn unblock_user(db: &Database, guild_row_id: &str, user_id: &str) -> Result<bool> {
    let result =
        sqlx::query("DELETE FROM discord_blocked_users WHERE guild_row_id = ? AND user_id = ?")
            .bind(guild_row_id)
            .bind(user_id)
            .execute(db.pool())
            .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn list_blocked_users(db: &Database, guild_row_id: &str) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT user_id FROM discord_blocked_users WHERE guild_row_id = ? ORDER BY created_at",
    )
    .bind(guild_row_id)
    .fetch_all(db.pool())
    .await?)
}

/// Set a server's channel name template.
pub async fn set_channel_template(
    db: &Database,
    guild_row_id: &str,
    kind: &str,
    template: &str,
) -> Result<()> {
    if !matches!(kind, "logs" | "control") {
        return Err(DatabaseError::Invalid(format!(
            "unknown channel kind `{kind}`"
        )));
    }

    sqlx::query(
        "INSERT INTO discord_channel_templates (guild_row_id, kind, template)
         VALUES (?, ?, ?)
         ON CONFLICT(guild_row_id, kind) DO UPDATE SET template = excluded.template",
    )
    .bind(guild_row_id)
    .bind(kind)
    .bind(template)
    .execute(db.pool())
    .await?;
    Ok(())
}

/// A server's templates as `(kind, template)` pairs. Absent kinds fall back to
/// the defaults in the `discord` crate rather than being written eagerly, so a
/// change of default reaches servers that never customised theirs.
pub async fn list_channel_templates(
    db: &Database,
    guild_row_id: &str,
) -> Result<Vec<(String, String)>> {
    let rows =
        sqlx::query("SELECT kind, template FROM discord_channel_templates WHERE guild_row_id = ?")
            .bind(guild_row_id)
            .fetch_all(db.pool())
            .await?;

    Ok(rows
        .into_iter()
        .map(|row| (row.get("kind"), row.get("template")))
        .collect())
}

/// Record the channels created for a project, together with the events that
/// should reach them, in one transaction.
///
/// Atomic because a channel row without its event rows is a project whose
/// channels exist and which silently reports nothing.
pub async fn record_channels(
    db: &Database,
    channels: &NewChannels,
    enabled_events: &[String],
) -> Result<()> {
    let now = time::now();
    let mut transaction = db.pool().begin().await?;

    sqlx::query(
        "INSERT INTO discord_project_channels
            (project_id, guild_row_id, logs_channel_id, control_channel_id,
             logs_channel_name, control_channel_name, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(project_id) DO UPDATE SET
             guild_row_id         = excluded.guild_row_id,
             logs_channel_id      = excluded.logs_channel_id,
             control_channel_id   = excluded.control_channel_id,
             logs_channel_name    = excluded.logs_channel_name,
             control_channel_name = excluded.control_channel_name,
             updated_at           = excluded.updated_at",
    )
    .bind(&channels.project_id)
    .bind(&channels.guild_row_id)
    .bind(&channels.logs_channel_id)
    .bind(&channels.control_channel_id)
    .bind(&channels.logs_channel_name)
    .bind(&channels.control_channel_name)
    .bind(&now)
    .bind(&now)
    .execute(&mut *transaction)
    .await?;

    sqlx::query("DELETE FROM discord_enabled_events WHERE project_id = ?")
        .bind(&channels.project_id)
        .execute(&mut *transaction)
        .await?;

    for event in enabled_events {
        sqlx::query("INSERT INTO discord_enabled_events (project_id, event_kind) VALUES (?, ?)")
            .bind(&channels.project_id)
            .bind(event)
            .execute(&mut *transaction)
            .await?;
    }

    transaction.commit().await?;
    Ok(())
}

pub async fn find_channels(db: &Database, project_id: &str) -> Result<Option<ChannelRecord>> {
    let row = sqlx::query(
        "SELECT project_id, guild_row_id, logs_channel_id, control_channel_id,
                logs_channel_name, control_channel_name, control_message_id,
                enabled, mention_role_on_failure, batch_window_ms
           FROM discord_project_channels WHERE project_id = ?",
    )
    .bind(project_id)
    .fetch_optional(db.pool())
    .await?;

    Ok(row.map(|row| ChannelRecord {
        project_id: row.get("project_id"),
        guild_row_id: row.get("guild_row_id"),
        logs_channel_id: row.get("logs_channel_id"),
        control_channel_id: row.get("control_channel_id"),
        logs_channel_name: row.get("logs_channel_name"),
        control_channel_name: row.get("control_channel_name"),
        control_message_id: row.get("control_message_id"),
        enabled: row.get::<i64, _>("enabled") != 0,
        mention_role_on_failure: row.get("mention_role_on_failure"),
        batch_window_ms: row
            .get::<i64, _>("batch_window_ms")
            .clamp(0, i64::from(u32::MAX)) as u32,
    }))
}

/// The events enabled for a project.
pub async fn list_enabled_events(db: &Database, project_id: &str) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT event_kind FROM discord_enabled_events WHERE project_id = ? ORDER BY event_kind",
    )
    .bind(project_id)
    .fetch_all(db.pool())
    .await?)
}

/// Replace the enabled event set for a project.
pub async fn set_enabled_events(db: &Database, project_id: &str, events: &[String]) -> Result<()> {
    let mut transaction = db.pool().begin().await?;

    sqlx::query("DELETE FROM discord_enabled_events WHERE project_id = ?")
        .bind(project_id)
        .execute(&mut *transaction)
        .await?;

    for event in events {
        sqlx::query("INSERT INTO discord_enabled_events (project_id, event_kind) VALUES (?, ?)")
            .bind(project_id)
            .bind(event)
            .execute(&mut *transaction)
            .await?;
    }

    transaction.commit().await?;
    Ok(())
}

/// Update the notification preferences that are not the event list.
///
/// `NULL` leaves a column alone, which is what lets the settings screen send
/// only what changed without this needing dynamic SQL.
pub async fn update_notification_settings(
    db: &Database,
    project_id: &str,
    enabled: Option<bool>,
    mention_role_on_failure: Option<Option<&str>>,
    batch_window_ms: Option<u32>,
) -> Result<()> {
    // The mention role is genuinely nullable, so "leave alone" and "clear it"
    // cannot both be expressed by NULL. It gets its own flag instead.
    let (set_mention, mention_value) = match mention_role_on_failure {
        None => (false, None),
        Some(value) => (true, value),
    };

    sqlx::query(
        "UPDATE discord_project_channels
            SET enabled                 = COALESCE(?, enabled),
                mention_role_on_failure = CASE WHEN ? THEN ? ELSE mention_role_on_failure END,
                batch_window_ms         = COALESCE(?, batch_window_ms),
                updated_at              = ?
          WHERE project_id = ?",
    )
    .bind(enabled.map(i64::from))
    .bind(i64::from(set_mention))
    .bind(mention_value)
    .bind(batch_window_ms.map(i64::from))
    .bind(time::now())
    .bind(project_id)
    .execute(db.pool())
    .await?;
    Ok(())
}

/// Remember which message carries the control panel, so it can be edited in
/// place instead of reposted.
pub async fn set_control_message(
    db: &Database,
    project_id: &str,
    message_id: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE discord_project_channels
            SET control_message_id = ?, updated_at = ? WHERE project_id = ?",
    )
    .bind(message_id)
    .bind(time::now())
    .bind(project_id)
    .execute(db.pool())
    .await?;
    Ok(())
}

/// Stop sending a project's events to Discord and forget its channels.
///
/// The channels themselves are left in place in Discord. Deleting a channel
/// full of history because someone unlinked a project would be an unpleasant
/// surprise, and the user can delete them by hand.
pub async fn forget_channels(db: &Database, project_id: &str) -> Result<bool> {
    let result = sqlx::query("DELETE FROM discord_project_channels WHERE project_id = ?")
        .bind(project_id)
        .execute(db.pool())
        .await?;
    Ok(result.rows_affected() > 0)
}
