# ADR 0009: Taint Tracking

- **状态:** Accepted
- **日期:** 2026-07-28
- **决策者:** VoicePilot Team
- **标签:** security, taint

## 背景

敏感数据(如 LLM 输入)可能被传播到非预期位置(文件系统、外部 MCP 工具)。如何追踪并阻止?

## 决策

使用 Taint Tracking 追踪敏感数据传播。

- `TaintRepo`:taints 表 + UNIQUE(value_hash) 索引(idempotent upsert)
- `compute_value_hash`:`canonicalize_json`(BTreeMap 字段排序)后 SHA256
- `check_taint_policy`:独立模块级函数,检查传播是否被阻止
- `tag_dag_plan_literals`:DAG 字面量标记 taint 来源
- 审计:`taint_propagated` / `taint_blocked`(隐私:只记 hash,不记原始值)

## 替代方案

- **无追踪(仅出站策略):** 拒绝。无法识别敏感数据流向。
- **全量数据流分析:** 拒绝。工程量大,V1 聚焦关键路径。

## 后果

- <正向:阻止敏感数据传播到非预期 sink>
- <负向:需在 dispatcher / MCP / filesystem 各点插入检查>
- <中性:审计只记 hash,保护隐私>

## 参考

- W9 设计文档 `docs/superpowers/specs/2026-07-28-w9-stronghold-taint-dagmodify-reale2e-design.md`
- `voicepilot/crates/trust-kernel/src/policy/taint_repo.rs`