# ADR 0005: MCP 版本锁定

- **状态:** Accepted
- **日期:** 2026-07-20
- **决策者:** VoicePilot Team
- **标签:** mcp, protocol

## 背景

VoicePilot 通过 MCP(Model Context Protocol)与外部工具 / 服务器通信。协议版本如何管理?

## 决策

锁定 MCP 协议版本 2025-11-25,`allow_version_fallback = false`。

- MCP server 支持 JSON-RPC 2.0 + NDJSON 帧解析(`mcp/transport.rs`)
- `McpServerRepo` 从 `mcp_servers` 表加载配置
- 版本偏移时拒绝而非回退,保证行为确定

## 替代方案

- **跟随最新 MCP 版本:** 拒绝。协议变动可能导致行为漂移。
- **允许版本回退:** 拒绝。隐藏协议不兼容,违反确定性。

## 后果

- <正向:协议行为确定,测试稳定>
- <负向:新 MCP 版本特性需手动升级评估>
- <中性:版本锁定在配置层,便于未来调整>

## 参考

- W4 设计文档 `docs/superpowers/plans/2026-07-20-w4-mcp-server-wrapping.md`
- `voicepilot/crates/trust-kernel/src/mcp/`