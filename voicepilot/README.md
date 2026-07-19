# VoicePilot

权限感知语音桌面 Agent — 单一 Rust 信任内核实现。

## 当前状态：W1（Rust Trust Kernel 骨架）

W1 范围：文本入口 + SQLite + 审计 + 状态机。无语音、无 MCP、无工具调用。

## 开发

```bash
cargo build
cargo test
cargo run -p cli
```

参考规格：`../voicepilot-v1.1-spec/voicepilot-v1.1-spec.html`
