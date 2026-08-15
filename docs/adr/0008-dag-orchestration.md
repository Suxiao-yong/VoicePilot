# ADR 0008: DAG 编排 + LLM 拆解

- **状态:** Accepted
- **日期:** 2026-07-26
- **决策者:** VoicePilot Team
- **标签:** orchestration, dag, llm

## 背景

复杂任务需要多步骤编排。如何组织任务为可审批、可执行的流程?

## 决策

使用 DAG(Directed Acyclic Graph)编排 + LLM 任务拆解。

- `skills/{template,dag_types,dag_repo,explanation_repo}.rs` 纯数据结构 + Repo
- `DagExecutor`:执行 DAG,支持骨架审批 + 节点级审批、循环节点、失败解释
- LLM 将用户意图拆解为 DAG(`decompose_to_dag` + `validate_dag`)
- 常量:`MAX_TOTAL_STEPS_HARD_LIMIT` / `MAX_LOOP_ITERATIONS_HARD_LIMIT`

## 替代方案

- **线性步骤序列:** 拒绝。无法表达分支 / 循环 / 依赖。
- **状态机全局:** 拒绝。复杂任务状态爆炸。
- **人工编排:** 拒绝。无法自动化。

## 后果

- <正向:灵活编排、可审批、可解释、失败可补偿>
- <负向:DAG 复杂度高,需校验(validate_dag)>
- <中性:与 slot 模板、approval 机制配合>

## 参考

- W8 设计文档 `docs/superpowers/specs/2026-07-26-w8-skill-orchestration-dag-design.md`
- `voicepilot/crates/trust-kernel/src/skills/{dag_executor,dag_types,dag_repo}.rs`