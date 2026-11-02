-- 契约 §2.1：voice_latency_samples 增加路由维度列（Task 2 可观测面板数据源）。
--
-- - route_kind: 路由通道标签，"voice"(默认/历史行) | "fastroute" | "keyword" | "llm" | "cache"
-- - cache_hit:  该次规划是否命中 route cache（0/1）
--
-- 历史行（008 写入的 voice 样本）由 DEFAULT 补齐，不需要数据回填。
-- compute_route_stats 按 route_kind 分组算 P50/P95/P99，idx_voice_latency_route
-- 加速 "某通道 + 时间窗口" 查询。
--
-- 幂等性：SQLite 不支持 `ADD COLUMN IF NOT EXISTS`，重复执行会报
-- "duplicate column name"。db.rs 用 003/005 同款「逐条执行 + 吞掉重复列错误」
-- 的方式注册本迁移；CREATE INDEX IF NOT EXISTS 本身幂等。

ALTER TABLE voice_latency_samples ADD COLUMN route_kind TEXT NOT NULL DEFAULT 'voice';

ALTER TABLE voice_latency_samples ADD COLUMN cache_hit INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_voice_latency_route ON voice_latency_samples(route_kind, started_at_ms);
