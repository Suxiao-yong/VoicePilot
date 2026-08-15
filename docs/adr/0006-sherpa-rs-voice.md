# ADR 0006: sherpa-rs 一站式语音

- **状态:** Accepted
- **日期:** 2026-07-20
- **决策者:** VoicePilot Team
- **标签:** voice, asr, tts

## 背景

VoicePilot 需要语音能力(ASR 识别 + TTS 合成 + VAD 检测)。选择何种语音方案?

## 决策

使用 sherpa-rs(`sherpa-rs` crate,`download-binaries` + `tts` feature)一站式语音。

- 语音模块:`voice/` 目录,分离 asr / tts / vad / audio / wav / model / model_download
- feature 门控:`voice` cargo feature,默认构建不引入 sherpa-rs
- W6b-3b 迁移后走预编译库,无需 CMake/bindgen

## 替代方案

- **whisper-rs 独立 ASR + 其他 TTS:** 拒绝。多依赖维护成本高。
- **系统级语音 API(Windows SAPI):** 拒绝。能力与跨模块一致性不足。
- **本地自研语音:** 拒绝。工程量大。

## 后果

- <正向:一站式 ASR/TTS/VAD,统一模型管理,预编译库免 CMake>
- <负向:sherpa-rs 依赖较大,模型需下载>
- <中性:feature 门控,默认构建无语音依赖>

## 参考

- W5 设计文档 `docs/superpowers/plans/2026-07-20-w5-voice-input.md`
- `voicepilot/crates/trust-kernel/src/voice/`