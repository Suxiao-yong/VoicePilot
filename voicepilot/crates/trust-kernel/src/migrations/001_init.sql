-- VoicePilot V1.1 initial schema (spec §8.1)
-- All tables created up-front; W1 only populates tasks/steps/audit_logs.

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS tasks (
    task_id            TEXT PRIMARY KEY,
    user_goal          TEXT NOT NULL,
    status             TEXT NOT NULL,           -- TaskState as SCREAMING_SNAKE_CASE
    skill_id           TEXT,                    -- V1.1: route metadata
    route_path         TEXT,                    -- V1.1: 'skill' | 'planner'
    transcript_hash    TEXT,
    created_at         TEXT NOT NULL,           -- RFC3339
    updated_at         TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS steps (
    step_id            TEXT PRIMARY KEY,
    task_id            TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
    step_order         INTEGER NOT NULL,
    tool_name          TEXT,
    args               TEXT,                    -- JSON
    args_hash          TEXT,
    status             TEXT NOT NULL,
    prepare_token      TEXT,                    -- V1.1
    preconditions_hash TEXT,                    -- V1.1
    effect_manifest    TEXT,                    -- V1.1 JSON
    evidence_strength  TEXT,                    -- V1.1: 'weak'|'medium'|'strong'
    compensation_ref   TEXT,                    -- V1.1
    egress_performed   INTEGER DEFAULT 0,       -- V1.1 bool
    started_at         TEXT,
    finished_at        TEXT
);

CREATE TABLE IF NOT EXISTS policies (
    policy_id          TEXT PRIMARY KEY,
    version            INTEGER NOT NULL,
    rules_json         TEXT NOT NULL,
    hash               TEXT NOT NULL,
    enabled            INTEGER NOT NULL DEFAULT 1,
    cedar_policies     TEXT,                    -- V1.1
    cedar_schema       TEXT,                    -- V1.1
    rust_constraints   TEXT                     -- V1.1
);

CREATE TABLE IF NOT EXISTS approvals (
    approval_id        TEXT PRIMARY KEY,
    task_id            TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
    step_id            TEXT REFERENCES steps(step_id) ON DELETE SET NULL,
    risk_level         TEXT,
    args_hash          TEXT,
    user_decision      TEXT,                    -- 'allow'|'deny'|'modify'
    decided_at         TEXT,
    E_level            TEXT,                    -- V1.1: 'E0'..'E3'
    D_level            TEXT,                    -- V1.1: 'D0'..'D3'
    destination        TEXT,                    -- V1.1
    egress_approved    INTEGER DEFAULT 0,       -- V1.1 bool
    approval_scope     TEXT,                    -- V1.1: 'single'|'batch'
    policy_bundle_hash TEXT                     -- V1.1
);

CREATE TABLE IF NOT EXISTS compensations (
    comp_id            TEXT PRIMARY KEY,
    step_id            TEXT NOT NULL REFERENCES steps(step_id) ON DELETE CASCADE,
    level              TEXT NOT NULL,           -- 'strong'|'best_effort'|'none'
    snapshot_encrypted BLOB,
    ttl_expires        TEXT,
    status             TEXT NOT NULL,
    compensation_level TEXT,                    -- V1.1 alias
    snapshot_vault_ref TEXT,                    -- V1.1
    conflict_policy    TEXT                     -- V1.1
);

CREATE TABLE IF NOT EXISTS audit_logs (
    log_id             TEXT PRIMARY KEY,
    task_id            TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
    step_id            TEXT REFERENCES steps(step_id) ON DELETE SET NULL,
    event_type         TEXT NOT NULL,
    details            TEXT NOT NULL,           -- JSON
    timestamp          TEXT NOT NULL,
    prev_hash          TEXT,
    hash               TEXT NOT NULL,
    otlp_trace_id      TEXT,                    -- V1.1
    otlp_span_id       TEXT,                    -- V1.1
    data_classification_redacted INTEGER DEFAULT 1  -- V1.1 bool
);

CREATE TABLE IF NOT EXISTS skills (
    skill_id           TEXT PRIMARY KEY,
    version            INTEGER NOT NULL,
    manifest_json      TEXT NOT NULL,
    enabled            INTEGER NOT NULL DEFAULT 1,
    success_count      INTEGER DEFAULT 0,
    avg_latency_ms     REAL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS taints (
    taint_id           TEXT PRIMARY KEY,
    value_hash         TEXT NOT NULL,
    provenance         TEXT NOT NULL,
    taints_json        TEXT NOT NULL,
    collected_at       TEXT NOT NULL,
    source_ref         TEXT
);

CREATE TABLE IF NOT EXISTS mcp_servers (
    server_id          TEXT PRIMARY KEY,
    name               TEXT NOT NULL,
    version            TEXT NOT NULL,
    transport          TEXT NOT NULL,
    enabled            INTEGER NOT NULL DEFAULT 1,
    trusted            INTEGER NOT NULL DEFAULT 0,
    protocol_version   TEXT,                    -- V1.1: locked to '2025-11-25'
    allowed_origins    TEXT,                    -- V1.1 JSON
    allowed_paths      TEXT                     -- V1.1 JSON
);

CREATE TABLE IF NOT EXISTS egress_log (
    egress_id          TEXT PRIMARY KEY,
    task_id            TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
    data_class         TEXT NOT NULL,           -- 'D0'..'D3'
    source             TEXT NOT NULL,
    destination        TEXT NOT NULL,
    approved           INTEGER NOT NULL DEFAULT 0,
    bytes              INTEGER DEFAULT 0,
    timestamp          TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_steps_task_id ON steps(task_id);
CREATE INDEX IF NOT EXISTS idx_audit_task_id ON audit_logs(task_id);
CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_logs(timestamp);
CREATE INDEX IF NOT EXISTS idx_approvals_task_id ON approvals(task_id);
