# VoicePilot — Rust Workspace

权限感知语音 Agent 的 Rust 信任内核与两种交付形态（桌面 Tauri / 无头 HTTP-SSE 网关）。
项目总览、功能、安全模型与实测数据见**仓库根目录的 [`../README.md`](../README.md)**。

## Crate 结构

| Crate | 作用 |
| --- | --- |
| `trust-kernel` | 单一信任边界：Cedar 策略 + E×D 风险矩阵 + Skill/DAG 执行 + MCP 客户端 + 补偿 + 哈希链审计 + 记忆/调度/延迟样本 |
| `ui` (`voicepilot-ui`) | Tauri 2 桌面壳：49 个命令、审批桥、桌宠、React 前端（`crates/ui/web`） |
| `cli` (`voicepilot`) | 二进制：`serve`（Axum HTTP/SSE 网关）、`eval`（评测 JSON I/O）、语音子命令 |

## 开发

```bash
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast -j 2     # Windows 中文断言需 chcp 65001
```

前端（`crates/ui/web`）：

```bash
npm install
npm test          # Vitest
npm run build     # tsc + vite build（dist/index.html 已提交，Tauri 编译期内嵌需要它）
```

## 两种运行方式

```bash
# 桌面（全功能：语音 + LLM + UIA + 内嵌前端）
cargo build --release -p voicepilot-ui --features voice,custom-protocol,uia
./target/release/voicepilot-ui.exe

# 无头网关（HTTP/SSE，共享同一 SQLite + Cedar + 审计链）
cargo build --release -p cli
./target/release/voicepilot serve --host 127.0.0.1 --port 8080 --db ./voicepilot.db
```

容器：`docker build -t voicepilot:latest .`（见本目录 `Dockerfile`）。

## 关键文档

- [`../docs/adr/`](../docs/adr/)：11 篇架构决策
- [`docs/`](docs/)：Skill 与扩展开发说明、MCP 插件示例
