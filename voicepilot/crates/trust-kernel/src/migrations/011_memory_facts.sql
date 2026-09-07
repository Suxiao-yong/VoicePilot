-- memory_facts 表：跨会话长期记忆（mem0 extract→consolidate→retrieve 最小子集）。
-- 只存从用户 turn 蒸馏出的事实（R2 finding 4：不从 tool result / web 内容提取）。
-- 矛盾不自动裁决：旧条 superseded_by 软删仅发生在明确"纠正"（category=correction
-- 且与旧条高重叠）时；检索时按新旧排序呈现。

CREATE TABLE IF NOT EXISTS memory_facts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    content TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'fact',
    source_turn TEXT NOT NULL DEFAULT '',
    created_at_ms INTEGER NOT NULL,
    last_hit_at_ms INTEGER NOT NULL,
    hit_count INTEGER NOT NULL DEFAULT 0,
    superseded_by INTEGER
);

CREATE INDEX IF NOT EXISTS idx_memory_facts_active ON memory_facts (superseded_by);
