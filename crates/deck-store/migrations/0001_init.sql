-- Launch Deck schema, first cut.
--
-- Conscious choices:
--
-- * `overrides`, `restart_policy` and `tags` are JSON documents in a column.
--   They are read and written only as part of the whole project row, never
--   queried by their internals, so a document-in-row keeps the schema honest
--   about how the data is actually used.
-- * Timestamps are RFC 3339 TEXT. SQLite has no datetime type, and an
--   explicit, readable format beats an integer that needs a comment.
-- * Log LINES are deliberately absent. Runs store a pointer to the log file
--   on disk; storing lines here would grow without bound (see ARCHITECTURE).

CREATE TABLE projects (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    description       TEXT,
    root              TEXT NOT NULL UNIQUE,
    -- what detection found (refreshed by re-detect, never edited by hand)
    runner_id         TEXT NOT NULL,
    language          TEXT NOT NULL,
    framework         TEXT,
    package_manager   TEXT,
    version           TEXT,
    detected_at       TEXT NOT NULL,
    -- what the user changed
    overrides         TEXT NOT NULL DEFAULT '{}',
    restart_policy    TEXT NOT NULL DEFAULT '{"kind":"never"}',
    -- organisation
    tags              TEXT NOT NULL DEFAULT '[]',
    category          TEXT,
    favorite          INTEGER NOT NULL DEFAULT 0,
    pinned            INTEGER NOT NULL DEFAULT 0,
    archived          INTEGER NOT NULL DEFAULT 0,
    notes             TEXT,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    last_launched_at  TEXT
);

CREATE TABLE runs (
    id           TEXT PRIMARY KEY,
    project_id   TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    lifecycle    TEXT NOT NULL,
    command      TEXT NOT NULL,
    started_at   TEXT NOT NULL,
    finished_at  TEXT,
    exit_code    INTEGER,
    outcome      TEXT NOT NULL,
    log_path     TEXT NOT NULL
);

CREATE INDEX idx_runs_project_started ON runs(project_id, started_at DESC);

CREATE TABLE settings (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);
