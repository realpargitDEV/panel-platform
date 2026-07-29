-- Discord integration.
--
-- Three things shape this schema, and all three are enforced by the file rather
-- than only by the code that writes to it:
--
-- 1. The bot token is a secret. There is no column it could be stored in
--    unencrypted — `discord_bot` has a ciphertext and a nonce and nothing else.
--    A future writer that wanted to store it in the clear would have to alter
--    the table to do it.
-- 2. Discord identifiers are TEXT, not INTEGER. They are 64-bit and routinely
--    exceed what a JavaScript number holds exactly; keeping them textual all
--    the way through means no layer can quietly round one.
-- 3. Permission levels and event kinds are constrained lists, checked against
--    their Rust enums by `schema_parity`.

-- The bot's own credentials. One row, like agent_state.
CREATE TABLE discord_bot (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    application_id TEXT NOT NULL,
    -- XChaCha20-Poly1305, same scheme as secret environment variables.
    token_cipher   BLOB NOT NULL,
    token_nonce    BLOB NOT NULL,
    updated_at     TEXT NOT NULL,
    CHECK (length(token_cipher) > 0),
    CHECK (length(token_nonce) = 24)
);

-- A Discord server the bot has been linked to.
CREATE TABLE discord_guilds (
    id                TEXT PRIMARY KEY,
    guild_id          TEXT NOT NULL UNIQUE,
    guild_name        TEXT NOT NULL,
    -- The account that linked the server. Always an administrator; see
    -- `permissions::AccessPolicy` for why this cannot be revoked here.
    linked_by_user_id TEXT NOT NULL,
    allow_guild_owner INTEGER NOT NULL DEFAULT 1 CHECK (allow_guild_owner IN (0, 1)),
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    CHECK (length(guild_id) > 0),
    CHECK (length(linked_by_user_id) > 0)
);

-- "This role or person may do this much."
CREATE TABLE discord_grants (
    id           TEXT PRIMARY KEY,
    guild_row_id TEXT NOT NULL REFERENCES discord_guilds(id) ON DELETE CASCADE,
    subject_kind TEXT NOT NULL CHECK (subject_kind IN ('role', 'user')),
    subject_id   TEXT NOT NULL,
    level        TEXT NOT NULL CHECK (level IN ('view', 'operate', 'administer')),
    created_at   TEXT NOT NULL,
    -- One grant per subject. Two rows for the same role would make "the highest
    -- wins" a property of insertion order.
    UNIQUE (guild_row_id, subject_kind, subject_id)
);

CREATE TABLE discord_blocked_users (
    guild_row_id TEXT NOT NULL REFERENCES discord_guilds(id) ON DELETE CASCADE,
    user_id      TEXT NOT NULL,
    reason       TEXT,
    created_at   TEXT NOT NULL,
    PRIMARY KEY (guild_row_id, user_id)
);

-- Per-server channel naming, so a user can rename every project's channels at
-- once rather than one at a time.
CREATE TABLE discord_channel_templates (
    guild_row_id TEXT NOT NULL REFERENCES discord_guilds(id) ON DELETE CASCADE,
    kind         TEXT NOT NULL CHECK (kind IN ('logs', 'control')),
    template     TEXT NOT NULL CHECK (length(template) BETWEEN 1 AND 100),
    PRIMARY KEY (guild_row_id, kind)
);

-- The two channels belonging to one project.
CREATE TABLE discord_project_channels (
    project_id              TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    guild_row_id            TEXT NOT NULL REFERENCES discord_guilds(id) ON DELETE CASCADE,
    logs_channel_id         TEXT NOT NULL,
    control_channel_id      TEXT NOT NULL,
    logs_channel_name       TEXT NOT NULL,
    control_channel_name    TEXT NOT NULL,
    -- The panel message, edited in place rather than reposted.
    control_message_id      TEXT,
    -- Mute switch, separate from the event list so that silencing a project
    -- during an incident does not discard its configuration.
    enabled                 INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    mention_role_on_failure TEXT,
    batch_window_ms         INTEGER NOT NULL DEFAULT 2000
                            CHECK (batch_window_ms BETWEEN 0 AND 60000),
    created_at              TEXT NOT NULL,
    updated_at              TEXT NOT NULL,
    -- Sending the control panel into the log channel would bury it under the
    -- logs within minutes.
    CHECK (logs_channel_id <> control_channel_id),
    UNIQUE (logs_channel_id),
    UNIQUE (control_channel_id)
);

CREATE INDEX idx_discord_channels_guild ON discord_project_channels (guild_row_id);

-- Which events reach Discord for a given project. A row per enabled event
-- rather than a delimited column, so the CHECK can constrain the values.
CREATE TABLE discord_enabled_events (
    project_id TEXT NOT NULL
               REFERENCES discord_project_channels(project_id) ON DELETE CASCADE,
    event_kind TEXT NOT NULL CHECK (event_kind IN (
        'started',
        'stopped',
        'crashed',
        'restarted',
        'deployment_started',
        'deployment_succeeded',
        'deployment_failed',
        'health_degraded',
        'health_recovered',
        'resource_warning',
        'backup_completed',
        'backup_failed',
        'error_logged',
        'warning_logged',
        'log_output'
    )),
    PRIMARY KEY (project_id, event_kind)
);
