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