-- W9 Plan 3: taints 表 value_hash UNIQUE 约束。
-- 防止并发 upsert 在 TaintRepo::upsert 的 ON CONFLICT(value_hash) DO UPDATE
-- 路径之外产生重复行(spec §6.2 taint tracking 安全约束)。
-- 幂等:CREATE UNIQUE INDEX IF NOT EXISTS 可重复执行。
CREATE UNIQUE INDEX IF NOT EXISTS idx_taints_value_hash ON taints(value_hash);
