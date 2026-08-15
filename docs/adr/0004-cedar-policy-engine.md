# ADR 0004: Cedar 策略引擎

- **状态:** Accepted
- **日期:** 2026-07-19
- **决策者:** VoicePilot Team
- **标签:** policy, security

## 背景

VoicePilot 需要策略引擎评估文件操作 / 命令执行等动作是否被允许。选择何种策略引擎?

## 决策

使用 AWS Cedar(`cedar-policy` crate)作为策略引擎。

- `policy/cedar_engine.rs`:Cedar 策略评估
- `policy/default.cedar`:默认策略(拒绝一切 + 显式放行)
- 配合 E×D 风险矩阵(`risk_matrix.rs`)、约束引擎(`constraint_engine.rs`)、出站策略(`egress.rs`)、事务(`transaction.rs`)
- Action Gateway:统一决策入口

## 替代方案

- **自研 DSL 策略:** 拒绝。维护成本高、无生态。
- **OPA / Rego:** 拒绝。引入重依赖,与 Rust 集成不如 Cedar 原生。
- **简单 if-else 白名单:** 拒绝。无法表达复杂权限模型。

## 后果

- <正向:声明式策略、形式化语义、与 Rust 集成良好>
- <负向:Cedar 学习曲线;策略需在 Rust 侧桥接>
- <中性:策略文件与代码分离,便于审计>

## 参考

- W2 设计文档 `docs/superpowers/plans/2026-07-19-w2-policy-action-gateway.md`
- `voicepilot/crates/trust-kernel/src/policy/`