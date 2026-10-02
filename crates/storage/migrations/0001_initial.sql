-- Bukno state, schema version 1: the nine Pass 1 records (specification section 6).
-- Identifiers are 32 lowercase hex digits. Times are UTC Unix seconds.

CREATE TABLE workspace (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL,
    -- Normalized path components as a JSON array, for overlap checks.
    key TEXT NOT NULL UNIQUE,
    -- "device:inode" of the folder, so two paths to one folder are one workspace.
    fs_identity TEXT,
    kind TEXT NOT NULL CHECK (kind IN ('project', 'chat')),
    git_root TEXT,
    -- The run that last took the writer lease and its engine process
    -- generation. Informational: leases are rebuilt from open runs at launch.
    writer_run TEXT,
    process_generation INTEGER,
    created_at INTEGER NOT NULL
);

CREATE TABLE project (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES workspace (id),
    archived INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE TABLE task (
    id TEXT PRIMARY KEY,
    project_id TEXT REFERENCES project (id),
    -- Reserved for delegation: the parent task.
    parent_id TEXT REFERENCES task (id),
    workspace_id TEXT NOT NULL REFERENCES workspace (id),
    provider TEXT NOT NULL CHECK (provider IN ('codex', 'claude')),
    title TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    archived INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE provider_session (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL UNIQUE REFERENCES task (id),
    -- Latest provider thread or session ID; resume can issue a new one.
    thread_id TEXT NOT NULL,
    engine_version TEXT,
    engine_path TEXT,
    resume_state TEXT NOT NULL DEFAULT 'resumable',
    latest_turn_id TEXT,
    updated_at INTEGER NOT NULL
);

CREATE TABLE run (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES task (id),
    attempt INTEGER NOT NULL,
    state TEXT NOT NULL,
    -- Effective settings snapshot as JSON: preset, writes, model, effort.
    settings TEXT NOT NULL,
    turn_id TEXT,
    -- The user stopped waiting for an unknown outcome.
    dismissed INTEGER NOT NULL DEFAULT 0,
    terminal_reason TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (task_id, attempt)
);

CREATE TABLE message_delivery (
    -- One current delivery record per message ID.
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES task (id),
    run_id TEXT REFERENCES run (id),
    draft_revision INTEGER NOT NULL,
    body TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('queued', 'about_to_send', 'sent', 'acknowledged', 'rejected', 'unknown')),
    provider_ack TEXT,
    updated_at INTEGER NOT NULL
);

CREATE TABLE draft (
    task_id TEXT PRIMARY KEY REFERENCES task (id),
    text TEXT NOT NULL,
    attachments TEXT NOT NULL DEFAULT '[]',
    revision INTEGER NOT NULL,
    saved_at INTEGER NOT NULL
);

CREATE TABLE pending_decision (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES run (id),
    connection_generation INTEGER NOT NULL,
    provider_request_id TEXT,
    -- Kind and requested scope as JSON.
    kind TEXT NOT NULL,
    allowed TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'sending', 'resolved', 'expired')),
    updated_at INTEGER NOT NULL
);

CREATE TABLE transcript_item (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES task (id),
    run_id TEXT REFERENCES run (id),
    -- Display order within the task, assigned on first insert. Real, so
    -- turns found later (continued outside Bukno) can go between two items.
    seq REAL NOT NULL,
    provider_item_id TEXT,
    kind TEXT NOT NULL CHECK (kind IN ('user', 'agent')),
    provider TEXT,
    text TEXT NOT NULL,
    meta TEXT,
    completed INTEGER NOT NULL,
    revision INTEGER NOT NULL,
    UNIQUE (task_id, seq)
);

CREATE UNIQUE INDEX transcript_item_provider ON transcript_item (task_id, provider_item_id)
    WHERE provider_item_id IS NOT NULL;
CREATE INDEX run_open ON run (state) WHERE state NOT IN ('completed', 'failed', 'interrupted');
CREATE INDEX decision_open ON pending_decision (state) WHERE state IN ('pending', 'sending');
