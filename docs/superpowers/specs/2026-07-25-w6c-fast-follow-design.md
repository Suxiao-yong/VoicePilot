# W6c Fast-Follow: W6b-3b 审查遗留项修复 Design Spec

**日期:** 2026-07-25(Asia/Shanghai)
**对应规格:** V1.1.2 §8.4 语音转写快速纠错 + VP-FR-002 语音反馈 TTS
**前置:** W6b-3b 已完成(commit `2759350`,236+48+78 测试通过)
**范围:** W6b-3b 最终代码审查发现的 P1/P2 遗留项修复(Windows-only,不做 macOS/Linux 打包)

---

## 1. 背景

W6b-3b 最终代码审查(subagent-driven-development 两阶段审查)发现 3 个 P0 + 多个 P1/P2 问题。3 个 P0 已修复(commit `7a7ea4c` / `78f16d2` / `34b9a89`),本 spec 处理剩余 P1/P2 项。

**P0 已修复(供参考):**
- P0 #1 TTS 实际播放音频(commit `7a7ea4c`)
- P0 #2 w6b3b_e2e_smoke.rs 缺失(commit `78f16d2`)
- P0 #3 voice_model_path 默认值(commit `34b9a89`)

---

## 2. 修复范围(7 项)

### 2.1 P1 #1 SettingsView TTS UI

**问题:** 后端 `SettingsDto` 有 `tts_enabled: bool` + `tts_model_path: String`,但前端 `types.ts` 的 `Settings` interface **没有**这两个字段;`SettingsView.tsx` 完全没有 TTS 配置 UI,用户无法在 UI 中开关 TTS 或修改 TTS 模型路径。

**修复:**
- `voicepilot/crates/ui/web/src/types.ts`: `Settings` interface 加 `tts_enabled: boolean` + `tts_model_path: string`
- `voicepilot/crates/ui/web/src/components/SettingsView.tsx`: 在"语音配置" fieldset 后新增"TTS 配置" fieldset:
  - `tts_enabled` checkbox(label "启用语音反馈")
  - `tts_model_path` text input(label "TTS 模型路径(空=使用默认)")
- 现有 `handleField<K extends keyof Settings>` 泛型已支持新字段,无需改逻辑

**测试:** 现有 `settings_commands_unit.rs` 已覆盖 TTS 字段往返;前端无单测,人工验证 UI 渲染 + 保存生效

### 2.2 P1 #2 Slot 提交重执行(手动 Apply 按钮)

**问题:** `MainView.tsx` 中 `SlotEditDialog` 提交后仅 `setSlots` 更新本地 state,未触发任何后端调用。§8.4 "不要求用户重复整句话,只补充缺失 Slot" 隐含修改后应继续执行,当前实现停留在"展示修改"层面。

**设计决策:** 手动 Apply 按钮(用户选择)。Slot 提交后只更新本地 state + 标记 `modified`,用户手动点 "Apply 修改" 按钮触发重新执行。

**修复:**
- `voicepilot/crates/ui/web/src/types.ts`: `Slot` interface 加 `modified?: boolean` 字段
- `voicepilot/crates/ui/web/src/components/Chip.tsx`: `modified=true` 时显示 "✓" 标记 + `.chip-modified` 样式
- `voicepilot/crates/ui/web/src/styles.css`: 新增 `.chip-modified` 样式(绿色边框 + ✓ 角标)
- `voicepilot/crates/ui/web/src/components/MainView.tsx`:
  - SlotEditDialog onSubmit:更新 slots state + 标记 `modified: true`
  - 新增 "Apply 修改" 按钮(`slots.some(s => s.modified)` 时显示)
  - Apply 逻辑:
    1. 按 `slot.end` 降序排序(从后往前替换避免偏移)
    2. 用 `slot.raw` 替换 transcription 中 `[start, end)` 区间
    3. 生成新文本 → 调 `routeText(newText)` → 更新 `routeResult`
    4. 清空所有 slot 的 `modified` 标记(不引入 applied 状态,保持简单)
- `voicepilot/crates/ui/web/src/types.ts`: Slot 加 `modified?: boolean`(可选字段,后端 Rust 不需要,仅前端状态)

**测试:** 人工验证流程:语音输入 → 转写 → Chip 修改 → Apply → 路由结果更新

### 2.3 P2 #1 VoiceError 文案

**问题:** `voice/error.rs` 第 13 行 `InferenceFailed` 错误文案仍写 "whisper inference failed",迁移到 sherpa-rs 后语义错误。

**修复:**
- `voicepilot/crates/trust-kernel/src/voice/error.rs`: `"whisper inference failed: {0}"` → `"inference failed: {0}"`
- `voicepilot/crates/trust-kernel/tests/voice_unit.rs`: 测试断言更新(`whisper inference failed` → `inference failed`)

**测试:** `cargo test -p trust-kernel --features voice voice_unit`(若 link 失败,在 ui crate voice feature 中验证)

### 2.4 P2 #2 TTS sample_rate

**问题:** `SherpaTtsConfig::default()` 设 `sample_rate: 16000`,但中文 VITS 模型实际输出通常为 22050 Hz。`voice_commands.rs` 用 config 值写 WAV 头,会导致播放速度/音调失真。

**调研:** sherpa-rs 0.6.8 `Audio` struct 是否暴露 `sample_rate` 字段(执行时查看 docs.rs 或源码)。

**方案 A(若 API 暴露 sample_rate):**
- `tts.rs`: `synth` 返回 `(Vec<i16>, u32)` 而非 `Vec<i16>`(samples + sample_rate)
- `SherpaTtsEngine` 加 `actual_sample_rate: AtomicU32` 字段,synth 时缓存实际值
- `voice_commands.rs`: `wav::write_wav` 用 `engine.actual_sample_rate()` 而非 `engine.config().sample_rate`

**方案 B(若 API 不暴露):**
- `SherpaTtsConfig::default` 的 `sample_rate` 从 16000 改为 22050(中文 VITS 常见值)
- 加注释说明用户需匹配模型原生采样率
- `voice_commands.rs` 逻辑不变

**测试:** 单元测试无法验证实际模型(需下载 ~200MB),人工验证播放音调正常

### 2.5 P2 #3 缓存失效扩展

**问题:** `voice_commands.rs` asr_cache / tts_cache 仅在 `model_dir` 变化时失效,改 `language` / `num_threads` / `speed` 等参数缓存不失效,新配置不生效。

**修复:**
- `voicepilot/crates/trust-kernel/src/voice/asr.rs`: `SherpaAsrConfig` 派生 `PartialEq`
- `voicepilot/crates/trust-kernel/src/voice/tts.rs`: `SherpaTtsConfig` 派生 `PartialEq`(注意:`sample_rate` 在方案 A 中由模型决定,参与比较会导致缓存频繁失效;排除 `sample_rate` 字段或用 `model_dir + num_threads + speed` 元组比较)
- `voicepilot/crates/ui/src/voice_commands.rs`:
  - asr_cache: `eng.config().model_dir != model_dir` → `eng.config() != &asr_config`
  - tts_cache: `eng.config().model_dir != model_dir` → 比较 `(model_dir, num_threads, speed)` 元组

**简化:** 为 `SherpaAsrConfig` / `SherpaTtsConfig` 派生 `PartialEq`,直接比较整个 config。TTS 的 `sample_rate` 在方案 B 中是配置值应参与比较;在方案 A 中是模型决定值不参与比较。

**测试:** 新增单测:改 `language` 后 cache 失效 + 重新加载

### 2.6 P2 #4 Push-to-talk emit 错误日志

**问题:** `app.rs` 第 22-24 行 `let _ = app.emit(...)` 吞掉 emit 错误,若主窗口未注册监听器或事件系统异常,用户按快捷键无任何反馈。

**修复:**
- `voicepilot/crates/ui/src/app.rs`:
  ```rust
  if let Err(e) = app.emit("push-to-talk-start", ()) {
      eprintln!("[voice] emit push-to-talk-start failed: {}", e);
  }
  ```
- 不引入 `tracing` 依赖(保持简单),用 `eprintln!`

**测试:** 人工验证(无法在单测中模拟 emit 失败)

### 2.7 P2 #5 CSP nonce — 跳过(用户选择 A)

**问题:** `tauri.conf.json` 的 CSP `style-src` 仍用 `'unsafe-inline'`。

**技术限制:** Tauri 2 的 CSP nonce 自动注入只对 `index.html` 中的 `<style>` / `<script>` 标签有效,**不对 React 运行时 `style={{...}}` prop 有效**。当前 `MainView.tsx` 等组件大量使用 `style={{ marginTop: 48 }}` 等 inline style prop,移除 `'unsafe-inline'` 会导致 UI 渲染失败。

**决策:** 跳过(用户选择 A)。在 `PROGRESS.md` 记录技术原因,W7+ 评估时再决定是否做"移除所有 React inline style prop"的重构。

---

## 3. 文件清单

### 修改文件
- `voicepilot/crates/ui/web/src/types.ts` — Settings + Slot interface 扩展
- `voicepilot/crates/ui/web/src/components/SettingsView.tsx` — TTS 配置 fieldset
- `voicepilot/crates/ui/web/src/components/Chip.tsx` — modified 标记
- `voicepilot/crates/ui/web/src/components/MainView.tsx` — Apply 按钮 + 重执行逻辑
- `voicepilot/crates/ui/web/src/styles.css` — .chip-modified 样式
- `voicepilot/crates/trust-kernel/src/voice/error.rs` — InferenceFailed 文案
- `voicepilot/crates/trust-kernel/src/voice/tts.rs` — sample_rate 修复 + PartialEq
- `voicepilot/crates/trust-kernel/src/voice/asr.rs` — SherpaAsrConfig PartialEq
- `voicepilot/crates/trust-kernel/tests/voice_unit.rs` — 测试断言更新
- `voicepilot/crates/ui/src/voice_commands.rs` — 缓存失效扩展
- `voicepilot/crates/ui/src/app.rs` — emit 错误日志

### 新增文件
- 无

### 文档
- `docs/PROGRESS.md` — W6c Fast-Follow 完成记录 + CSP nonce 跳过说明

---

## 4. 测试策略

### 单元测试
- `voice_unit.rs`: 更新 InferenceFailed 文案断言
- `tts.rs` / `asr.rs`: 新增 `PartialEq` 比较测试 + 缓存失效测试
- `w6b3b_e2e_smoke.rs`: 现有 6 个测试不回归

### 集成测试
- `cargo test --workspace --no-default-features` — 236 → 236+(无回归,新增 PartialEq 测试)
- `cargo test -p voicepilot-ui --features voice` — 78 → 78+(新增缓存失效测试)
- `cargo test -p voicepilot-ui --features tauri` — 48 无回归
- `cargo clippy --workspace --no-default-features -- -D warnings` — 0 warnings
- `npm.cmd run build` — PASS

### 人工验证
- Settings 页面 TTS 配置 UI 渲染 + 保存生效
- 语音输入 → Chip 修改 → Apply 按钮 → 路由结果更新
- TTS 播放音调正常(需下载 TTS 模型)
- Push-to-talk 快捷键 emit 失败时 stderr 有日志

---

## 5. 已知偏离 / 延后项

- **CSP nonce 跳过:** React inline style prop 限制,W7+ 评估重构
- **macOS/Linux 打包:** 用户明确"Windows-only",不做
- **trust-kernel voice feature link.exe 内存失败:** 环境限制,非代码问题,ui crate voice feature 已覆盖

---

## 6. 提交规范

- `feat(w6c): ...` / `fix(w6c): ...` / `refactor(w6c): ...` / `test(w6c): ...` / `docs(w6c): ...`
- 每个 P1/P2 项一个 commit(7 个 commit + 1 个 docs commit = 8 个)
- PowerShell `;` 分隔,`git commit -m "msg"` 单行

---

## 7. 验收门禁

- ✅ `cargo test --workspace --no-default-features` 236+ passing,0 failed
- ✅ `cargo test -p voicepilot-ui --features voice` 78+ passing,0 failed
- ✅ `cargo test -p voicepilot-ui --features tauri` 48 passing,0 failed
- ✅ `cargo clippy --workspace --no-default-features -- -D warnings` 0 warnings
- ✅ `npm.cmd run build` PASS
- ✅ SettingsView TTS 配置 UI 可见 + 可保存
- ✅ Chip 修改后 Apply 按钮触发 routeText 重新路由
- ✅ VoiceError 文案为 "inference failed"(无 "whisper")
- ✅ TTS 播放音调正常(若模型可用)
- ✅ Push-to-talk emit 失败时 stderr 有日志
