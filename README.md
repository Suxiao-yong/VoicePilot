# VoicePilot

> VoicePilot is a permission-aware desktop voice agent (Windows). One sentence gets things done — download a song, clip its chorus, organize files, launch apps — with risk assessment, approval and full audit behind every action.

VoicePilot 是一个权限感知的 Windows 桌面语音 Agent：一句话办事（搜歌 → 下载 → 截副歌；整理文件；开关应用），所有副作用经过风险评估、审批确认与审计留痕。云端 LLM 只做理解与规划，执行永远走本地信任内核。

---

## 功能亮点

- **一句话办事**：打字、语音、桌宠气泡三种入口；“下载晴天”“截这首歌的副歌”“整理下载目录”直达执行。
- **三级路由**：确定性快路由（零 token、毫秒级）→ 关键词路由 → LLM 分类/拆解（DAG），接不住的自动回退，绝不硬猜。
- **审批分级**：仅改/删文件与高危操作（`shell.run`、`form.submit`）弹审批，其余 50+ 技能免审批直行、审计留痕。
- **多轮记忆**：实时快照（10 秒新鲜度门禁）+ 每轮封轮摘要 + 跨会话偏好蒸馏（可看可删）。
- **追问卡**：歌名含糊时三选一，超时自动选默认，不卡死。
- **后台作业**：进程内调度器（“每天早上八点总结”），漏跑不追补，结果进任务历史。
- **信任壳**：Cedar 策略引擎 + E×D 风险矩阵 + 哈希链审计 + 污染溯源 + Stronghold 加密快照；MCP 工具调用强制白名单与 schema 基线。
- **60+ 内置 Skill**：文件 / 系统 / 剪贴板 / 网页 / 媒体 / 邮件 / 日历 / 定时作业 / 副歌截取……，另支持用户自定义 Skill 与外部 MCP 导入（密钥进系统凭据库，不落明文）。

## 架构

```
用户（语音 / 文字 / 桌宠）
  │  Tauri 2 Shell（主窗口 + 桌宠 + 托盘）/ React 前端
  │  IPC（invoke + event；WebView 不直访文件与 MCP）
  ▼
信任内核 Trust Kernel（Rust，单一信任边界）
  ├─ 感知：VAD → 转写 → RealtimeSnapshot → 记忆召回
  ├─ 规划：快路由 → 关键词 → LLM classify → LLM 拆 DAG
  ├─ 决策：Cedar + E×D 矩阵 + 审批 + 追问卡
  ├─ 执行：Skill 分发 / MCP 客户端 / UIA 自动化
  └─ 善后：强校验 → 补偿 → 哈希链审计 → 污染溯源
```

关键原则：LLM、前端、外部 MCP 都在信任壳之外——只能提议，不能执行；任何不确定都失败闭合（fail-closed），返回可读错误。

## 技术栈

| 层 | 技术 |
|---|---|
| 内核 | Rust（tokio、rusqlite、serde）、Cedar 策略、SQLite（含 FTS 检索位） |
| 桌面 | Tauri 2（tray-icon、global-shortcut、dialog）、WebView2 |
| 前端 | React 18 + TypeScript + Vite + Vitest |
| 语音 | sherpa-rs（ASR/TTS/VAD，opt-in feature） |
| 外部集成 | Playwright MCP、Composio MCP（邮件试点）、mcp-windows（UIA 后端） |
| 工程 | GitHub Actions CI、cargo-deny 供应链、rustdoc + TypeDoc、changeset |

## 快速开始

### 前置要求

- Rust stable（`rustup` 最新）、Node.js 18+、Windows 10+（WebView2，一般自带；缺席时安装包自动引导下载）。
- 仅 `voice` feature 需要：MSVC Build Tools + CMake 3.20+ + libclang（sherpa-rs 构建依赖），以及约 2GB 磁盘给模型。
- 首次运行建议联网：yt-dlp / ffmpeg 缺席时自动供给，语音模型首次下载。

### 构建

```powershell
cd voicepilot

# 全功能桌面应用（推荐）：语音 + LLM + UIA + 内嵌前端
cargo build -p voicepilot-ui --features voice,custom-protocol,uia

# 最小构建（纯内核逻辑，无语音/TAUI）
cargo build -p trust-kernel --no-default-features
```

> ⚠️ `custom-protocol` 必开：Tauri 2 按该特性（而非 debug/release）决定窗口加载内嵌前端还是 dev 服务器。不开则双击 exe 显示 `localhost 拒绝连接`。详见 `voicepilot/crates/ui/Cargo.toml` 注释。

产物：`voicepilot/target/debug/voicepilot-ui.exe`（release 同理）。直接双击运行；配置与数据落在 `%APPDATA%\voicepilot`（不会进仓库）。

### 配置

1. 启动后进 **Settings** 页填云端 LLM API Key（保存即生效，重启自动重建；密钥进系统凭据库）。
2. 语音模型首次在 ModelDownloadBar 下载（sherpa 系列）。
3. 外部 Skill / MCP：在 Skills Manager 与 Trust Center 里扫描导入（默认不启用、不信任，需手动开）。

### 试试

- `整理下载目录` → files.organize（需一批确认）
- `下载晴天` → 搜歌下载（输出声明假设版本）
- `截这首歌的副歌` → 副歌定位 + 截 60 秒（需 python + pychorus + ffmpeg）
- `打开记事本` → 应用控制（需 uia feature 构建）

## 测试

```powershell
cd voicepilot

# Rust 全量（默认 + uia 双套件；中文用例需 UTF-8 代码页）
chcp 65001
cargo test -p trust-kernel --lib
cargo test -p trust-kernel --features uia --lib
cargo test --workspace --no-default-features -j 2

# 静态检查（两个组合都要干净）
cargo clippy -p trust-kernel -p voicepilot-ui --no-default-features -- -D warnings
cargo clippy -p trust-kernel -p voicepilot-ui --features uia,llm -- -D warnings

# 前端
cd crates/ui/web
npm install
npm test        # Vitest
npx tsc --noEmit
```

说明：shell/剪贴板中文断言依赖控制台代码页（936 下乱码属环境问题）；`--test-threads=1` 可用于定位并行干扰；评测基建见 `evals/`（Inspect AI / promptfoo）。

## 目录结构

```
voicepilot/                  # Rust workspace
  crates/
    trust-kernel/            # 信任内核：policy / skills / mcp / voice / dag / audit / memory / scheduler
    ui/                      # Tauri 应用：commands / approver / voice / web（React 前端）
    cli/                     # 命令行入口（含评测子命令）
  docs/…                     # （随内核仓）
docs/
  adr/                       # 架构决策记录（11 篇：平台/LLM/加密/策略/MCP/语音/UI/DAG/污染/评测/供应链）
  PROGRESS.md                # 里程碑进度记录
  release.md                 # 发版说明
evals/                       # 评测任务集与基线
tools/mcp-windows/           # UIA 后端（本地下载，不入仓）
sherpa-libs/                 # 语音运行时（本地下载，不入仓）
```

## 安全模型（摘要）

- 执行白名单：未知 skill_id 在快照门直接拒绝，执行器/MCP 进程跑不起来。
- 密钥卫生：外部凭据只存引用（`$keyring:`），值永不进 renderer、不进日志、不进导出审计之外的任何文本。
- 隐私模式：关闭记忆写入与 LLM 上下文注入，降级为关键词/无匹配。
- 补偿：删文件类操作登记 reverse 函数，`task.compensate` 可回滚。

## 文档

- `docs/adr/`：每项关键决策的背景 / 方案 / 否决项 / 后果。
- `docs/PROGRESS.md`：W1–W12b 里程碑与测试统计。
- `voicepilot/docs/`：Skill 与扩展开发说明。

## 许可证

暂缺 LICENSE 文件（默认保留所有权利）。公开发布前请补（如 MIT / Apache-2.0）。
