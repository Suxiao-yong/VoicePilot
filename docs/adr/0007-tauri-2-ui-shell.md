# ADR 0007: Tauri 2 UI Shell

- **状态:** Accepted
- **日期:** 2026-07-21
- **决策者:** VoicePilot Team
- **标签:** ui, tauri

## 背景

VoicePilot 需要桌面 UI shell(审批窗口、设置、聊天等)。选择何种 UI 框架?

## 决策

使用 Tauri 2 作为 UI shell。

- `voicepilot-ui` crate,`tauri` cargo feature 门控(TauriApprover / commands)
- React + TypeScript 前端(`crates/ui/web`)
- IPC 强制三条安全规则:WebView 不能直接访问文件系统、UI 不能直接调 MCP、approval_request_id 单次使用
- TauriApprover 用 oneshot channel + 5min timeout(默认 Deny)

## 替代方案

- **Electron:** 拒绝。体积大、内存占用高。
- **纯 Web(浏览器):** 拒绝。无法访问本地桌面能力与系统集成。
- **原生 Win32/WPF:** 拒绝。生态与开发效率不足。

## 后果

- <正向:轻量、Rust 后端 + Web 前端、安全 IPC>
- <负向:Tauri 生态需适配;沙箱打包环境受限>
- <中性:feature 门控,默认构建无 Tauri 依赖>

## 参考

- W6a 设计文档 `docs/superpowers/plans/2026-07-21-w6a-tauri-shell-approval.md`
- `voicepilot/crates/ui/`