-- W10 Plan 3: voice_latency_samples 表(spec §VP-FR-001 P95 ≤ 500ms 基准)。
--
-- 存储每次 voice listen 的延迟样本:
-- - started_at_ms: VAD 检测首个 voiced chunk 的 epoch ms(t0)
-- - latency_ms: t1 - t0(t1 = 首个 partial transcript 回调)
-- - model: sherpa-rs 模型名(如 "sense_voice")
-- - privacy_mode: 0=cloud LLM, 1=local only
--
-- compute_stats(since_ms) 查询 WHERE started_at_ms >= since_ms,
-- idx_voice_latency_started 索引加速。
--
-- 清理策略:prune_older_than(days) 删除 started_at_ms < (now - days*86400*1000) 的行,
-- 默认保留 30 天(spec §5.2 v2 修订 #15)。由 CLI `voice latency-prune --days 30` 触发。
--
-- 幂等性:CREATE TABLE IF NOT EXISTS / CREATE INDEX IF NOT EXISTS,可重复执行。

CREATE TABLE IF NOT EXISTS voice_latency_samples (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at_ms INTEGER NOT NULL,
    latency_ms INTEGER NOT NULL,
    model TEXT NOT NULL,
    privacy_mode INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_voice_latency_started ON voice_latency_samples(started_at_ms);
