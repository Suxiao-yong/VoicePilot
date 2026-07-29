-- W9 Plan 2: 落实 compensations 表的 reverse_payload + compensate_fn 真实列。
-- 之前 W3a PoC 把这两个字段以 JSON stash 在 snapshot_vault_ref 列(repo.rs:37-48),
-- 导致 spec §2.2 明文残留检测 SQL `WHERE reverse_payload != ''` 无法直接执行。
-- 本 migration 拆出真实列,并把 W3a stash 数据迁移过去,清空 snapshot_vault_ref。

ALTER TABLE compensations ADD COLUMN reverse_payload TEXT DEFAULT '';
ALTER TABLE compensations ADD COLUMN compensate_fn TEXT DEFAULT '';

-- 注意:不在此处做数据迁移(UPDATE reverse_payload),由应用层 hook
-- (db.rs::migrate_005_compensations_stash) 精确解析 stash JSON 后填充
-- reverse_payload + compensate_fn,避免中间状态数据不一致。
