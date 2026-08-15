# ADR 0010: Inspect AI + promptfoo 评测

- **状态:** Accepted
- **日期:** 2026-08-06
- **决策者:** VoicePilot Team
- **标签:** eval, testing

## 背景

VoicePilot 需要评测基础设施验证 V1 验收门禁(功能 / 攻击 / TOCTOU / 恶意 Server / 数据安全)。选择何种评测框架?

## 决策

使用 Inspect AI(functional 100 任务)+ promptfoo(redteam 50 攻击)+ Rust smoke 测试(20 TOCTOU + 15 恶意 Server + 20 数据安全)。

- `evals/` 目录:functional / redteam / toctou / malicious_server / data_security / scorers
- `run_all.sh` 串联 5 项评测,生成 `summary.json` + `v1_gate_verdict`
- `inspect_evals.py`(Inspect AI)+ `promptfooconfig.yaml`(promptfoo)
- 5 个 Fitness Functions 强制评测集完整性(`w11_default_boundary_smoke.rs`)

## 替代方案

- **单一框架:** 拒绝。不同评测类型(LLM 任务 / 攻击 / 系统级)需要不同工具。
- **纯手工验证:** 拒绝。无法回归。

## 后果

- <正向:5 项评测基础设施闭合 V1 验收门禁,可自动化>
- <负向:依赖 OpenAI API key,评测运行 60min>
- <中性:CI 中 evals 仅 tag 触发>

## 参考

- W11 设计文档 `docs/superpowers/specs/2026-08-06-w11-eval-infrastructure-design.md`
- `evals/` 目录 + `docs/PROGRESS.md` §九