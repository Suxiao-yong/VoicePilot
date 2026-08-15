# ADR 0002: 永久云端 LLM only

- **状态:** Accepted
- **日期:** 2026-07-19
- **决策者:** VoicePilot Team
- **标签:** llm, cloud

## 背景

VoicePilot 需要 LLM 能力(任务拆解、失败解释、路由决策)。部署方式如何选择?

## 决策

VoicePilot 永久使用云端 LLM only,不支持本地 LLM(ollama / llama.cpp 等)。

- LLM 客户端通过 `ureq` / `reqwest` 调用云端 API
- `voice` / `llm` feature 门控,默认构建不引入本地 LLM 依赖
- 无 ollama / llama.cpp 集成

## 替代方案

- **本地 LLM(ollama / llama.cpp):** 拒绝。隐私好但质量、速度、模型版本管理成本高,且用户设备异构。
- **混合(云端 + 本地回退):** 拒绝。V1 聚焦云端,避免双路径维护。

## 后果

- <正向:质量稳定,维护简单,无本地推理依赖(CPU/GPU 异构)>
- <负向:依赖网络与云端 API key,存在调用成本>
- <中性:LLM 调用计费 / 速率限制延后到 V1.1+>

## 参考

- spec §1.1 平台范围
- `voicepilot/crates/trust-kernel/src/llm/` 模块