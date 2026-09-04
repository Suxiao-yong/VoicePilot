-- turns 表：语音/文本每轮封轮记忆（只存摘要与结果，不存音频与原文长文本）。

CREATE TABLE IF NOT EXISTS turns (
    turn_id TEXT PRIMARY KEY,
    started_at_ms INTEGER NOT NULL,
    source TEXT NOT NULL,
    transcript TEXT NOT NULL DEFAULT '',
    outcome TEXT NOT NULL DEFAULT '',
    plan_id TEXT NOT NULL DEFAULT '',
    latency_ms INTEGER NOT NULL DEFAULT 0,
    sensitive INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_turns_started ON turns (started_at_ms);
