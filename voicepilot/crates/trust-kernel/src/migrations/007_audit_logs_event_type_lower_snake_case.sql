-- W10 清理项 3:audit_logs.event_type 历史数据 SCREAMING_SNAKE_CASE → lower_snake_case。
--
-- spec §10 Conventions 要求所有 audit event_type 必须 lower_snake_case。
-- W1-W8 遗留 14 种 SCREAMING_SNAKE_CASE 事件在 W10 已全部重命名为 lower_snake_case
-- (源码层 emit callsite 已改),本 migration 转换已有 audit_logs 行的 event_type 列,
-- 保证历史数据与新代码一致,避免 w9_default_boundary_smoke::audit_event_types_all_lower_snake_case
-- 测试在带历史数据的库上失败。
--
-- 幂等性:lower_snake_case 值不匹配任何 SCREAMING_SNAKE_CASE WHEN 子句,
-- 重复执行不会再次转换,UPDATE 影响 0 行。
--
-- 不使用 ALTER TABLE —— event_type 列类型已是 TEXT,无需 schema 变更。

UPDATE audit_logs SET event_type = CASE event_type
    WHEN 'TASK_CREATED' THEN 'task_created'
    WHEN 'STEP_CREATED' THEN 'step_created'
    WHEN 'STEP_STATUS_CHANGED' THEN 'step_status_changed'
    WHEN 'STEP_PREPARED' THEN 'step_prepared'
    WHEN 'STEP_COMMITTED' THEN 'step_committed'
    WHEN 'STEP_STARTED' THEN 'step_started'
    WHEN 'STEP_SUCCEEDED' THEN 'step_succeeded'
    WHEN 'STEP_FAILED' THEN 'step_failed'
    WHEN 'STATE_TRANSITION' THEN 'state_transition'
    WHEN 'COMPENSATION_CREATED' THEN 'compensation_created'
    WHEN 'COMPENSATION_STATUS_CHANGED' THEN 'compensation_status_changed'
    WHEN 'APPROVAL_RECORDED' THEN 'approval_recorded'
    WHEN 'MCP_TOOLS_CALL' THEN 'mcp_tools_call'
    WHEN 'MCP_CALL_FAILED' THEN 'mcp_call_failed'
    ELSE event_type
END;
