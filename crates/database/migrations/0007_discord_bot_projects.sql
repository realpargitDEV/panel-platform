-- Which projects a bot covers.
--
-- Distinct from `discord_project_channels`, and deliberately so. That table
-- records a project's *actual* channels — two Discord channel ids that exist
-- because the bot created them in a linked server. This one records the user's
-- choice, which comes first and survives on its own:
--
--   "this bot reports on these projects"
--
-- Keeping the two apart means a user can decide what a bot covers before any
-- server is linked, and it means channel provisioning has something to read
-- when a server is linked later. Folding the choice into the channels table
-- would have made it impossible to express until channels existed, which is
-- backwards — the choice is the input, the channels are the result.
CREATE TABLE discord_bot_projects (
    bot_row_id TEXT NOT NULL REFERENCES discord_bots(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    -- One row per pair. Selecting a project twice is the same statement made
    -- twice, not two different ones.
    PRIMARY KEY (bot_row_id, project_id)
);

-- Both directions are queried: the Discord screen asks "what does this bot
-- cover", and a project's own screen asks "which bots report on this". The
-- primary key serves the first; this index serves the second.
CREATE INDEX idx_discord_bot_projects_project ON discord_bot_projects (project_id);
