-- More than one Discord bot.
--
-- 0002 stored the bot's credentials in a one-row table, `discord_bot`, on the
-- assumption that an installation has a bot the way it has an `agent_state`.
-- That assumption is what this migration removes: a person running several
-- servers, or separating a private bot from one their community can see, needs
-- each connection to be its own thing with its own token and its own on/off.
--
-- Three properties from 0002 are preserved deliberately:
--
-- 1. There is still no column a token could be stored in unencrypted. The
--    ciphertext and nonce columns carry over unchanged, including the length
--    checks, so the guarantee survives the widening.
-- 2. Discord identifiers stay TEXT.
-- 3. The existing row is carried forward, not dropped. Someone who configured
--    a bot before this build keeps it, and keeps its linked servers, without
--    re-entering a token they may no longer have a copy of.

-- The bots this installation knows about. Many rows, unlike `discord_bot`.
CREATE TABLE discord_bots (
    id             TEXT PRIMARY KEY,
    -- What the user calls it in the window. Their label, not Discord's, so a
    -- connection is recognisable before it has ever connected and learned its
    -- own name.
    label          TEXT NOT NULL,
    application_id TEXT NOT NULL UNIQUE,
    -- XChaCha20-Poly1305, same scheme and same AAD as before.
    token_cipher   BLOB NOT NULL,
    token_nonce    BLOB NOT NULL,
    -- Whether this connection starts with the application. Off by default:
    -- a connection the user has not asked to run should not begin running
    -- because the machine rebooted.
    autostart      INTEGER NOT NULL DEFAULT 0 CHECK (autostart IN (0, 1)),
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    CHECK (length(label) > 0),
    CHECK (length(application_id) > 0),
    CHECK (length(token_cipher) > 0),
    CHECK (length(token_nonce) = 24)
);

-- Carry the single existing bot forward, if there is one.
--
-- The id is a literal rather than a generated one because SQL cannot generate
-- this schema's prefixed ids, and because a readable marker is worth more here
-- than uniformity: anyone inspecting the file by hand can see which row predates
-- the widening. `created_at` is unknown for a row that never recorded one, so
-- `updated_at` stands in for both rather than inventing a time.
INSERT INTO discord_bots
    (id, label, application_id, token_cipher, token_nonce, autostart, created_at, updated_at)
SELECT
    'bot_carried_forward_from_v5',
    'Discord bot',
    application_id,
    token_cipher,
    token_nonce,
    0,
    updated_at,
    updated_at
FROM discord_bot
WHERE id = 1;

-- Which bot a linked server belongs to.
--
-- Added rather than rebuilt: 0003 and 0004 rebuilt because SQLite cannot alter
-- an existing CHECK, and nothing here alters one. A nullable column with a
-- reference is something ADD COLUMN supports directly.
--
-- Nullable on purpose. A server linked while no bot was configured has no
-- honest answer for this column, and NULL says that plainly where a fabricated
-- reference would not. Such a row is adopted when a bot is attached to it.
ALTER TABLE discord_guilds
    ADD COLUMN bot_row_id TEXT REFERENCES discord_bots(id) ON DELETE CASCADE;

-- Every server linked before this migration belonged to the only bot there was.
-- The UPDATE is unambiguous for exactly that reason, and stops being available
-- the moment a second bot exists.
UPDATE discord_guilds
   SET bot_row_id = 'bot_carried_forward_from_v5'
 WHERE EXISTS (SELECT 1 FROM discord_bots WHERE id = 'bot_carried_forward_from_v5');

CREATE INDEX idx_discord_guilds_bot ON discord_guilds (bot_row_id);

-- The one-row table has no remaining readers.
DROP TABLE discord_bot;
