# ADR 0001: 永久 Windows-only

- **状态:** Accepted
- **日期:** 2026-07-19
- **决策者:** VoicePilot Team
- **标签:** platform

## 背景

VoicePilot 是权限感知的语音桌面 Agent,依赖 Windows 原生能力(语音 / UIA 应用控制 / NSIS 打包)。是否需要跨平台支持?

## 决策

VoicePilot 永久 Windows-only。所有代码、CI、打包均针对 Windows 目标。

- `deny.toml [graph] targets = ["x86_64-pc-windows-msvc"]`
- UIA 适配器 `#[cfg(all(windows, feature = "uia"))]` 编译期守卫
- GitHub Actions 全用 `windows-latest` runner
- 无 `tauri.macos.conf.json`,NSIS 仅 Windows

## 替代方案

- **跨平台(Rust 天然可移植):** 拒绝。语音(sherpa-rs)、UIA、NSIS 均 Windows-centric,跨平台会显著增加维护成本。
- **macOS/Linux 最低支持:** 拒绝。无需求,且 UIA/NSIS 无法在非 Windows 实现。

## 后果

- <正向:专注 Windows 单一 target,配置与 CI 简化,依赖供应链检查仅针对 x64-msvc>
- <负向:无法覆盖其他平台用户,某些通用 crate 能力被锁定在 Windows>
- <中性:代码中 `#[cfg(windows)]` 守卫保留,未来若需跨平台可逐步放开>

## 参考

- spec §1.1 平台范围
- `voicepilot/Cargo.toml` workspace 配置