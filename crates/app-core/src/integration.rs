//! The composition layer for the Discord integration.
//!
//! Three crates meet here and nowhere else:
//!
//! * `database` stores rows and knows nothing about permissions or encryption.
//! * `discord` decides who may do what and what a message says, and knows
//!   nothing about storage.
//! * `security` holds the key.
//!
//! Joining them is deliberately a separate, small module rather than a
//! convenience method on either side. It is the only place a stored `String`
//! becomes a `Permission`, and the only place a stored blob becomes a usable
//! bot token — which makes both easy to find and to review.

use std::collections::BTreeSet;

use project_host_database::discord as storage;
use project_host_database::{Database, DatabaseError};
use project_host_discord::channels::{ChannelKind, NameTemplate};
use project_host_discord::events::NotificationSettings;
use project_host_discord::permissions::{AccessPolicy, Grant, Permission, Subject};
use project_host_discord::snowflake::Snowflake;
use project_host_discord::EventKind;
use project_host_security::{decrypt, encrypt, Ciphertext, EncryptionKey, Secret};

#[derive(Debug, thiserror::Error)]
pub enum IntegrationError {
    #[error("database error: {0}")]
    Database(#[from] DatabaseError),
    #[error("stored Discord id is not usable: {0}")]
    BadId(#[from] project_host_discord::snowflake::SnowflakeError),
    #[error("stored permission level `{0}` is not one this build understands")]
    UnknownLevel(String),
    #[error("stored grant subject `{0}` is not one this build understands")]
    UnknownSubject(String),
    #[error("could not decrypt the bot token; the key may have changed")]
    Decrypt,
    #[error("could not encrypt the bot token")]
    Encrypt,
    #[error("no Discord server has been linked with id {0}")]
    NoSuchGuild(String),
}

/// Additional data bound into the bot token's encryption.
///
/// Binding the ciphertext to its purpose means a blob lifted from this row
/// cannot be decrypted as though it were an environment variable, even with the
/// right key.
const BOT_TOKEN_AAD: &[u8] = b"project-host:discord-bot-token";

/// Encrypt and store the bot token.
///
/// The plaintext arrives as a [`Secret`], so it cannot be logged on the way in,
/// and leaves this function as ciphertext that the storage layer cannot read.
pub async fn save_bot_token(
    db: &Database,
    key: &EncryptionKey,
    application_id: &str,
    token: &Secret<String>,
) -> Result<(), IntegrationError> {
    let ciphertext = encrypt(key, token, BOT_TOKEN_AAD).map_err(|_| IntegrationError::Encrypt)?;

    storage::save_bot_credentials(
        db,
        &storage::BotCredentials {
            application_id: application_id.to_string(),
            token_cipher: ciphertext.bytes,
            token_nonce: ciphertext.nonce,
        },
    )
    .await?;
    Ok(())
}

/// Load and decrypt the bot token.
///
/// Returns `Ok(None)` when no bot has been configured, which is a normal state
/// and not an error — the integration is optional.
pub async fn load_bot_token(
    db: &Database,
    key: &EncryptionKey,
) -> Result<Option<(String, Secret<String>)>, IntegrationError> {
    let Some(stored) = storage::load_bot_credentials(db).await? else {
        return Ok(None);
    };

    let token = decrypt(
        key,
        &Ciphertext {
            bytes: stored.token_cipher,
            nonce: stored.token_nonce,
        },
        BOT_TOKEN_AAD,
    )
    .map_err(|_| IntegrationError::Decrypt)?;

    Ok(Some((stored.application_id, token)))
}

/// Assemble the access policy for a linked server from its stored rows.
///
/// Unknown values are refused rather than skipped. Silently dropping a grant
/// whose level this build does not recognise would quietly reduce someone's
/// permissions; silently dropping a *block* would quietly restore access to
/// somebody who had been shut out, which is worse. Refusing means an
/// unrecognised row is a loud failure that stops the interaction.
pub async fn access_policy_for(
    db: &Database,
    guild_id: &str,
) -> Result<Option<AccessPolicy>, IntegrationError> {
    let Some(guild) = storage::find_guild(db, guild_id).await? else {
        return Ok(None);
    };

    let mut policy = AccessPolicy::new(guild.linked_by_user_id.parse::<Snowflake>()?);
    policy.allow_guild_owner = guild.allow_guild_owner;

    for record in storage::list_grants(db, &guild.id).await? {
        let level = match record.level.as_str() {
            "view" => Permission::View,
            "operate" => Permission::Operate,
            "administer" => Permission::Administer,
            other => return Err(IntegrationError::UnknownLevel(other.to_string())),
        };
        let id = record.subject_id.parse::<Snowflake>()?;
        let subject = match record.subject_kind.as_str() {
            "role" => Subject::Role { id },
            "user" => Subject::User { id },
            other => return Err(IntegrationError::UnknownSubject(other.to_string())),
        };
        policy.grants.push(Grant { subject, level });
    }

    for blocked in storage::list_blocked_users(db, &guild.id).await? {
        policy.blocked_users.push(blocked.parse::<Snowflake>()?);
    }

    Ok(Some(policy))
}

/// The notification settings for a project, or `None` if it is not linked.
pub async fn notification_settings_for(
    db: &Database,
    project_id: &str,
) -> Result<Option<NotificationSettings>, IntegrationError> {
    let Some(channels) = storage::find_channels(db, project_id).await? else {
        return Ok(None);
    };

    let mut enabled_events = BTreeSet::new();
    for stored in storage::list_enabled_events(db, project_id).await? {
        // An event kind this build does not know is skipped rather than
        // refused: the consequence is one notification not being sent, which is
        // preferable to the whole integration failing after a downgrade.
        if let Some(kind) = EventKind::parse(&stored) {
            enabled_events.insert(kind);
        } else {
            tracing::warn!(event = %stored, "ignoring an unrecognised Discord event kind");
        }
    }

    let mention_role_on_failure = match channels.mention_role_on_failure {
        Some(role) => Some(role.parse::<Snowflake>()?),
        None => None,
    };

    Ok(Some(NotificationSettings {
        enabled_events,
        mention_role_on_failure,
        batch_window_ms: channels.batch_window_ms,
        enabled: channels.enabled,
    }))
}

/// The channel name templates for a server, with defaults filling any gaps.
///
/// A template that is stored but no longer parses falls back to the default
/// rather than failing: the worst case is a channel with a boring name, and
/// refusing would block creating the project at all.
pub async fn channel_templates_for(
    db: &Database,
    guild_row_id: &str,
) -> Result<Vec<(ChannelKind, NameTemplate)>, IntegrationError> {
    let stored = storage::list_channel_templates(db, guild_row_id).await?;

    Ok(ChannelKind::ALL
        .iter()
        .map(|kind| {
            let template = stored
                .iter()
                .find(|(stored_kind, _)| stored_kind == kind.as_str())
                .and_then(|(_, text)| NameTemplate::parse(text).ok())
                .unwrap_or_else(|| NameTemplate::default_for(*kind));
            (*kind, template)
        })
        .collect())
}
