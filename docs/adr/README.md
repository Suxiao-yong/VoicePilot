# ADR 索引

VoicePilot 架构决策记录(Architecture Decision Records)。每个 ADR 记录一个关键架构决策的背景 / 决策 / 替代方案 / 后果。

## 状态

| 编号 | 决策 | 状态 | 日期 | 标签 |
|---|---|---|---|---|
| [0001](0001-voicepilot-windows-only.md) | 永久 Windows-only | Accepted | 2026-07-19 | platform |
| [0002](0002-cloud-llm-only.md) | 永久云端 LLM only | Accepted | 2026-07-19 | llm, cloud |
| [0003](0003-stronghold-vault-encryption.md) | Stronghold Vault 加密快照 | Accepted | 2026-07-28 | crypto, security |
| [0004](0004-cedar-policy-engine.md) | Cedar 策略引擎 | Accepted | 2026-07-19 | policy, security |
| [0005](0005-mcp-version-locked.md) | MCP 版本锁定 | Accepted | 2026-07-20 | mcp, protocol |
| [0006](0006-sherpa-rs-voice.md) | sherpa-rs 一站式语音 | Accepted | 2026-07-20 | voice, asr, tts |
| [0007](0007-tauri-2-ui-shell.md) | Tauri 2 UI Shell | Accepted | 2026-07-21 | ui, tauri |
| [0008](0008-dag-orchestration.md) | DAG 编排 + LLM 拆解 | Accepted | 2026-07-26 | orchestration, dag, llm |
| [0009](0009-taint-tracking.md) | Taint Tracking | Accepted | 2026-07-28 | security, taint |
| [0010](0010-inspect-ai-promptfoo-eval.md) | Inspect AI + promptfoo 评测 | Accepted | 2026-08-06 | eval, testing |
| [0011](0011-cargo-deny-supply-chain.md) | cargo-deny 供应链安全 | Accepted | 2026-08-06 | supply-chain, security |

## 新增 ADR 流程

1. 复制 `0000-template.md` 为 `000X-<slug>.md`
2. 填写背景 / 决策 / 替代方案 / 后果 / 参考
3. 更新本索引表
4. 跑 `cargo test --test w12_default_boundary_smoke` 验证 ADR 完整性