# VoicePilot V1 发布手册

> **W12 Plan 5** — NSIS 打包 + 发布流程。

## 概述

VoicePilot V1 通过 GitHub Actions `release.yml` 在 tag `v*` 触发时构建 NSIS 安装包并上传 GitHub Release。

- **平台:** Windows-only(ADR 0001)
- **打包:** Tauri 2 NSIS installer(含 WebView2 bootstrapper)
- **签名:** V1 不做 Authenticode 代码签名(见下方"签名说明")

## 发布流程

### 1. 准备版本号

确认 `voicepilot/Cargo.toml` 的 `version` 与 `voicepilot/crates/ui/tauri.conf.json` 的 `version` 一致。

### 2. 打 tag

```powershell
cd d:\voicepilot
git tag v0.1.0
git push origin v0.1.0
```

### 3. 触发 CI

Push tag 后自动触发 `.github/workflows/release.yml` 的 `build-nsis` job:

1. 安装 Rust + Node
2. 安装 tauri-cli
3. `cargo tauri build --target nsis` 生成安装包
4. 上传到 GitHub Release(draft 状态)

### 4. 发布 GitHub Release

1. 在 GitHub Releases 页面打开 draft release
2. 附加 release notes(见 §验证清单)
3. 点击发布,使安装包公开

### 5. 手动安装测试

1. 下载并双击 `.exe` 安装包
2. 跟随安装向导(WebView2 bootstrapper 自动引导运行时)
3. 启动 VoicePilot,验证基本功能(语音 / 文件整理 / 审批)

## 签名说明

V1 不做 Authenticode 代码签名(ADR 0001 / spec §8.4 偏离):

- EV 代码签名证书成本高($200+/年)
- SmartScreen 可能对未签名安装包显示警告,release notes 中标注
- 代码签名留 V1.1+(spec §10.1)

## Release Notes 模板

```markdown
# VoicePilot v0.1.0

> **注意:** 本安装包未签名,SmartScreen 可能显示安全警告。点击"更多信息"→"仍要运行"即可。

## 新增
- ...

## 修复
- ...

## 已知问题
- ...

## 验证
- [ ] 100 功能任务(单步 ≥ 95% / 多步 ≥ 80%)
- [ ] 50 攻击样本(拦截 ≥ 95%)
- [ ] 20 TOCTOU(0 成功)
- [ ] 15 恶意 Server(0 绕过)
- [ ] 20 数据安全(0 未确认外发)
```

## 回滚

- 若新版本有严重问题,在 GitHub Releases 保留上一版本安装包,用户在官网下载旧版
- 代码回滚:revert tag 并重新打 tag 触发 CI

## 参考

- spec §10.1 安装与升级
- spec §八 Plan 5 NSIS 打包
- `.github/workflows/release.yml`
- `voicepilot/crates/ui/tauri.conf.json`

## V1 发布门禁状态（2026-08-19，内核平台化 Wave 5 后）

> 只有对应命令有**新鲜退出码和结果**才勾选；未满足项保持未勾选。

- [x] `cargo test --workspace --all-targets -j 1` — 全绿（含 rmcp parity 隔离测试）
- [x] `cargo check --workspace --features voice,tauri,llm,uia,stronghold -j 1` — exit 0
- [x] `cargo clippy --workspace --all-targets --features voice,tauri,llm,uia,stronghold -j 1 -- -D warnings` — exit 0（逐项修复真实死代码，无全局 allow）
- [x] `cargo test -p cli --all-targets --features voice -j 1` — 8 passed（含 voice_list_models_smoke 回归）
- [x] `npm test -- --run` / `npm run build` / `npm run lint` — 17 tests + build + lint 全绿
- [x] 20 TOCTOU — `cargo test -p trust-kernel --test w11_toctou_block_smoke -j 1` → 20/20, exit 0
- [x] 15 恶意 Server — `cargo test -p trust-kernel --test w11_malicious_server_smoke -j 1` → 15/15, exit 0
- [x] 20 数据安全 — `cargo test -p trust-kernel --test w11_data_security_smoke -j 1` → 20/20, exit 0
- [x] Configuration-only 扩展验收（范围收窄，如实记录）：新增只读 MCP Plugin → reload catalog → 文本/voice 经统一 PlannerPipeline 路由；MCP 链 dispatch 与 builtin 成功 dispatch 的 audit 断言 extension 元数据（`manifest_hash` / `source` / `snapshot_id`，MCP 另有 `server_id` / `tool_name`，见 `dispatch_snapshot_gate` / `mcp_plugin_config`）；policy hash 记录于 `approval_recorded` 审计事件；result status 即 step/tool 执行状态。完整成功执行 + 全字段断言的独立验收用例待后续补
- [ ] 100 功能任务（Inspect AI）— 需要 Python venv + inspect-ai 工具链，本机未运行
- [ ] 50 攻击样本（promptfoo redteam）— 需要 promptfoo 工具链，本机未运行
- [ ] 真实语音 P95 首字延迟 ≤ 500 ms（`cargo test -p trust-kernel --features voice --test w10_voice_latency_smoke -- --ignored --nocapture`）— 需要安装实际 SenseVoice 模型（约 1 GB）+ WAV fixture；本机无模型，测试执行后跳过（exit 0，skip），未关闭门禁