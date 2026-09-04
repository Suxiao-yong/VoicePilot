-- llm_route_cache 表：classify 高置信 Skill 命中的结果缓存。
-- key = sha256(模型 + schema版本 + 归一化文本)，含上下文时天然带上文；
-- 只存 Skill 命中（Dag/Unmatched 不进缓存，计划与候选清单会变）；
-- privacy 路径到不了写入点（上游已返回 Unmatched），无需额外门禁。

CREATE TABLE IF NOT EXISTS llm_route_cache (
    cache_key TEXT PRIMARY KEY,
    skill_id TEXT NOT NULL,
    slots_json TEXT NOT NULL DEFAULT '[]',
    confidence REAL NOT NULL,
    created_at_ms INTEGER NOT NULL,
    expires_at_ms INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_route_cache_expiry ON llm_route_cache (
    expires_at_ms
);
