# VoicePilot

![CI](https://github.com/Suxiao-yong/VoicePilot/actions/workflows/ci.yml/badge.svg)

> VoicePilot is a permission-aware desktop voice agent for Windows. Speak or type one sentence — download a song, clip its chorus, organize files, launch apps — every side effect passes risk assessment, user approval and hash-chained audit. The cloud LLM only understands and plans; execution always goes through the local trust kernel.

VoicePilot 是一个**权限感知**的 Windows 桌面语音 Agent。说一句话（或打一行字），搜歌、下载、截副歌、整理文件、开关应用一气呵成——每个副作用背后都有风险评估、审批确认与审计留痕。云端 LLM 只负责理解与规划，**执行永远走本地信任内核**。

- 高频指令走确定性快路由：零 token、毫秒级响应，不烧钱。
- 高危操作才弹审批：改/删文件、`shell.run`、不可逆提交；其余 50+ 技能免审批直行、审计留痕。
- 多轮对话有记忆：实时快照 + 每轮封轮摘要 + 跨会话偏好蒸馏（可看可删）。
- 拿不准就追问：歌名含糊弹三选一，超时自动选默认，不卡死。

---

## 目录

- [功能详解](#功能详解)
- [架构](#架构)
- [Skill 清单](#skill-清单)
- [技术栈](#技术栈)
- [快速开始](#快速开始)
- [构建矩阵](#构建矩阵)
- [配置说明](#配置说明)
- [使用指南](#使用指南)
- [测试](#测试)
- [目录结构](#目录结构)
- [安全模型](#安全模型)
- [文档索引](#文档索引)
- [已知限制与路线图](#已知限制与路线图)
- [许可证](#许可证)

---

## 功能详解

### 一句话办事（三种入口）

主窗口输入框、麦克风语音（VAD 自动断句 + Push-to-talk 全局热键）、桌宠气泡，同一套路由与执行。

```
整理下载目录          → files.organize（整理归档，一批一次确认）
下载晴天              → media.download（歌名搜源下载，声明假设版本）
截这首歌的副歌        → media.clip_chorus（副歌定位，截 60 秒）
打开记事本            → quick.app_control（应用控制）
每天早上八点总结      → task.schedule（后台定时作业）
搜到多个版本时        → 追问卡三选一（超时回默认）
```

### 三级路由（快 → 准 → 聪明）

1. **原子快路由**（`fastroute`）：正则一步完成意图匹配 + 参数提取，零 LLM 开销；形状不对一律回退，不挤掉正常流量。
2. **关键词路由**：Skill 自带的 keywords / intent_examples 确定性命中。
3. **LLM 路由**：classify 定技能抽参数（高置信命中进 SQLite 缓存 24h，复述零 HTTP）；复杂任务拆成 DAG（节点拓扑执行 + 骨架审批 + 失败归因）。

### 记忆（两层）

- **情景记忆**：每轮语音/文字封轮一条摘要（`turns` 表，原文音频永不落盘，30 天 TTL，隐私模式跳过）；下一轮快照自动带上文，LLM 可回指“还是刚才那个”。
- **长期记忆**：会话结束从用户发言蒸馏 ≤5 条事实（偏好/约定/纠正），相似合并、矛盾软删不 auto 裁决；召回 top-3 注入并声明来源。`memory.view` / `memory.forget` 给用户否决权。

### 后台作业

进程内调度器秒级 tick 扫 `agent_jobs` 表，到期走同一套 planner 执行，结果进任务历史 + 桌面通知。语义写死：**进程不在期间的漏跑不追补**（与 OpenClaw 一致）；DAG 内每步照走审批白名单，用户不在屏默认 Deny。

### 媒体链路（下载副歌一分钟）

歌名 → `ytsearch` 找源下载（top-3 标题消歧，明确胜出直下）→ pychorus 定位副歌 → 截取 → `ffmpeg -i` 验时长（±5s）。已知时间戳优先走 `trim_video`；pychorus 只是启发式 fallback，判错可能如实声明。

---

## 架构

```
用户（语音 / 文字 / 桌宠）
  │  Tauri 2 Shell（主窗口 + 桌宠 + 托盘）/ React 前端
  │  IPC（invoke + event；WebView 不直访文件系统与 MCP）
  ▼
信任内核 Trust Kernel（Rust，单一信任边界）
  ├─ 感知：VAD → 转写 → RealtimeSnapshot（10s 新鲜度门禁）→ 记忆召回
  ├─ 规划：快路由 → 关键词 → LLM classify → LLM 拆 DAG
  ├─ 决策：Cedar 策略 + E×D 风险矩阵 + 审批 + 追问卡
  ├─ 执行：Skill 分发 / MCP 客户端 / UIA 自动化
  └─ 善后：强校验 → 补偿 → 哈希链审计 → 污染溯源
        ▲ 云端 LLM（只理解规划）   ▲ Stronghold 加密   ▲ SQLite 落库
```

**一句话走完全程的例子**（“下载晴天”）：

1. `parse_download` 命中 → `{url: ytsearch1:晴天}`（零 token）。
2. executor 拉 top-3 标题，唯一命中直下，否则追问卡。
3. yt-dlp 下载（缺席自动供给 + SHA 校验），输出声明“默认下载《…》”。
4. 落 task/step + 审计；可补偿（删文件）。

**核心不变量**：LLM、前端、外部 MCP 都在信任壳之外——只能提议，不能执行；未知 skill_id 在快照门直接拒绝，执行器进程都起不来；任何不确定都失败闭合（fail-closed），返回可读错误。

---

## Skill 清单

| 组 | Skill（* 需对应 feature/外部程序） |
|---|---|
| 文件 | `files.organize`、`fs.read_file`、`fs.list_dir`、`fs.write_file`、`fs.create_file`、`fs.delete_file` |
| 系统 | `sys.datetime`、`sys.timer_set`、`sys.alarm_set`、`sys.volume`、`sys.playback`、`sys.lock_screen`、`sys.quit_all`、`sys.quit_browsers`、`sys.frontmost`、`sys.diagnose_app`、`sys.open_url`、`sys.maps_search`、`shell.run` |
| 剪贴板 | `clip.write`、`clip.read`、`clip.type_text`、`clip.press_keys`、`clip.save_image`、`clip.selected`、`clip.selected_files` |
| 网页 | `web.search`、`web.scrape`、`web.fetch_file`、`web.weather`、`web.sports`、`web.wallpapers`、`web.page_navigate/click/fill/screenshot`（*需 Playwright MCP） |
| 媒体 | `media.download`、`media.trim_video`、`media.convert_video`、`media.clip_chorus`（*需 python + pychorus + ffmpeg）、`doc.convert`、`doc.office` |
| 信息 | `mail.compose`、`mail.list_recent`/`mail.draft`（*需 Composio）、`pim.note_create`、`pim.notes_search`、`pim.reminder_create`、`pim.calendar_create/list` |
| 应用 | `quick.app_control`、`note.capture`（*需 uia） |
| 任务 | `task.explain`、`task.repeat_verified`、`task.compensate`、`task.schedule/jobs/unschedule`、`memory.view/forget`、`form.prepare/submit`、`research.save_markdown` |

另支持**用户自定义 Skill**（标准 `SKILL.md` 目录，YAML frontmatter，可覆盖内置）与**外部 MCP 导入**（Claude/Cursor/VSCode/Codex 配置，默认不启用不信任，密钥进系统凭据库）。

---

## 技术栈

| 层 | 技术 |
|---|---|
| 内核 | Rust（tokio、rusqlite、serde）、Cedar 策略引擎、SQLite |
| 桌面 | Tauri 2（tray-icon、global-shortcut、dialog）、WebView2 |
| 前端 | React 18 + TypeScript + Vite + Vitest + Testing Library |
| 语音 | sherpa-rs 预编译包（ASR/TTS/VAD，无需 CMake） |
| 外部集成 | Playwright MCP、Composio MCP（邮件试点）、mcp-windows（UIA 后端） |
| 评测 | Inspect AI、promptfoo、TOCTOU/红队/恶意 Server 场景集（`evals/`） |
| 工程 | GitHub Actions（CI/audit/docs/release）、cargo-deny、rustdoc + TypeDoc、changeset |

架构决策见 [`docs/adr/`](docs/adr/)（11 篇：平台、LLM 策略、加密、策略引擎、MCP 版本、语音、UI、DAG、污染追踪、评测、供应链）。

---

## 快速开始

### 前置要求

- Rust stable（`rustup`）、Node.js 18+、Windows 10+（WebView2 一般自带；安装包缺席会自动引导下载）。
- 语音/模型与 yt-dlp/ffmpeg 首次使用时自动供给，需要联网；pychorus 走 pip（`==` 锁定版本）。
- 磁盘：构建缓存较大（`target/` 可达百 GB 级，`.gitignore` 已覆盖；`cargo clean` 可随时清掉重编）。

### 构建运行

```powershell
cd voicepilot

# 全功能桌面应用（推荐）：语音 + LLM + UIA + 内嵌前端
cargo build --release -p voicepilot-ui --features voice,custom-protocol,uia
.\target\release\voicepilot-ui.exe
```

> ⚠️ `custom-protocol` 必开：Tauri 2 按该特性（而非 debug/release）决定窗口加载内嵌前端还是 dev 服务器。不开则双击 exe 显示 `localhost 拒绝连接`。

首次启动：进 **Settings** 填云端 LLM API Key（保存即生效，重启自动重建；密钥进系统凭据库）；语音模型在 ModelDownloadBar 下载。配置与数据落在 `%APPDATA%\voicepilot`，不进仓库。

---

## 构建矩阵

| 命令 | 说明 |
|---|---|
| `cargo build -p trust-kernel --no-default-features` | 纯内核逻辑（无 LLM/语音/UIA），CI 最快门 |
| `cargo build -p trust-kernel --features voice,llm` | 内核 + 语音 + LLM（路由/DAG 全功能） |
| `cargo build -p trust-kernel --features uia` | + Windows 应用控制 |
| `cargo build -p trust-kernel --features stronghold` | + 加密快照 |
| `cargo build -p voicepilot-ui --features voice,custom-protocol,uia` | 全功能桌面 exe（默认已含 llm） |
| `cargo build -p voicepilot-ui --features tauri` | 仅 UI 壳（无语音，编译最快） |

前端单独构建：`cd voicepilot/crates/ui/web && npm install && npm run build`（`web/dist/index.html` 已提交，Tauri 编译期内嵌需要它）。

---

## 配置说明

| 项 | 位置/方式 |
|---|---|
| LLM Key、开关 | 应用内 Settings 页（持久化 KV + 系统凭据库） |
| 数据库路径 | 默认运行目录 `voicepilot.db`，可用 `VOICEPILOT_DB` 环境变量覆盖 |
| 用户 Skill | `%APPDATA%\voicepilot\skills\<id>\SKILL.md`（boot 时捆绑技能缺席自动播种） |
| MCP Server | `mcp_servers` 表 + Trust Center UI；外部导入默认禁用 + 不信任 |
| 隐私模式 | Settings 开关：关闭记忆写入与 LLM 上下文注入 |

---

## 使用指南

- **整理文件**：`整理下载目录`（按类型归档，一批一次确认，可撤销）。
- **听歌**：`下载孤勇者`（声明假设版本）→ `截这首歌的副歌`（60 秒±5s）；含糊歌名弹三选一。
- **办公**：`打开记事本` / `记一条买菜笔记到 D:/todo.txt` / `新建会议纪要`。
- **网页**：`打开抖音搜索世界杯`（站内直达，零 LLM）/ `把这页存成 markdown`。
- **定时**：`每天早上八点总结`（作业列表可查可停）。
- **查账**：Audit Viewer 按任务查哈希链；`task.explain` 问失败归因。

---

## 测试

```powershell
cd voicepilot
chcp 65001   # 中文断言需 UTF-8 代码页，否则 shell/剪贴板用例乱码（环境问题非回归）

cargo test -p trust-kernel --lib
cargo test -p trust-kernel --features uia --lib
cargo test --workspace --no-default-features -j 2   # 链接器省内存

cargo clippy -p trust-kernel -p voicepilot-ui --no-default-features -- -D warnings
cargo clippy -p trust-kernel -p voicepilot-ui --features voice,uia,llm -- -D warnings

cd crates/ui/web && npm test && npx tsc --noEmit
```

测试分层：纯函数单测 → python-mock MCP 全链路 → wiremock 挡云端 → 真机冒烟（麦克风/真 OAuth 留给手工清单，见各功能节）。

---

## 目录结构

```
voicepilot/                        # Rust workspace
  crates/
    trust-kernel/                  # 信任内核
      src/
        approval/                  # Approver trait + 审批类型（含追问语义分离）
        policy/                    # Cedar 引擎 + E×D 矩阵 + 事务 effect
        skills/                    # 按域拆分：sys/clip/fs/web/shell/media/pim/doc
        │                         # + fastroute（原子快路由）/ dispatcher / dag_executor
        │                         # + manifest / router / reverse_fns（补偿）
        mcp/                       # JSON-RPC 传输 + Server 调度 + 客户端 + repo
        voice/                     # VAD/录音/转写/模型供给/路由桥
        uiautomation/              # UIA 适配（外部 mcp-windows 后端）
        compensation/              # 补偿类型 + reverse 注册表
        memory.rs scheduler.rs secrets.rs external_scan.rs  # 记忆/作业/密钥/扫描
        turns.rs llm_cache.rs planner.rs  # 情景记忆/分类缓存/规划管线
      tests/                       # 集成冒烟（dag/红队/toctou/恶意server/数据安全…）
    ui/                            # Tauri 应用：commands / approver / voice / scheduler / pet
      web/src/                     # React：MainView/ApprovalModal/ClarificationDialog/信任中心…
    cli/                           # 命令行（含评测子命令）
docs/
  adr/                             # 11 篇架构决策记录
  PROGRESS.md                      # 里程碑进度记录
  release.md                       # 发版说明
evals/                             # 评测任务集与语音基线
tools/mcp-windows/                 # UIA 后端（二进制，本地下载，不入仓）
sherpa-libs/                       # 语音运行时（二进制，本地下载，不入仓）
```

---

## 安全模型

- **执行白名单**：未知/禁用/仅展示 skill 在快照门直接拒绝，执行器与 MCP 进程跑不起来。
- **MCP 纵深**：transport 白名单（stdio）+ 协议版本锁定 + 工具 schema 基线 + 调用走 `mcp_tool_call_checked` + 污染溯源（`mcp_tool:<server_id>` 进审计）。
- **密钥卫生**：外部凭据只存 `$keyring:` 引用；值永不进 renderer、不进日志；缺失 fail-closed。
- **隐私模式**：记忆零写入、LLM 零注入，降级为关键词路由。
- **补偿与追溯**：删文件/清表单类操作登记 reverse 函数；审计哈希链防篡改；记忆注入声明来源。

---

## 文档索引

- [`docs/adr/`](docs/adr/)：架构决策（背景 / 方案 / 否决项 / 后果）。
- [`docs/PROGRESS.md`](docs/PROGRESS.md)：W1–W12b 里程碑与测试统计。
- [`docs/release.md`](docs/release.md)：发版流程（NSIS + WebView2 bootstrapper）。
- [`voicepilot/docs/`](voicepilot/docs/)：Skill 与扩展开发说明、MCP 插件示例。
- [`evals/`](evals/)：评测任务集与语音基线。

---

## 已知限制与路线图

- 调度器是进程内的：退出期间的漏跑不追补；要 OS 级可靠再上 schtasks（v1 不做）。
- 记忆蒸馏依赖 LLM 质量，记错可用 `memory.forget` 删；矛盾事实不自动裁决。
- Composio 邮件链路依赖第三方云；备选自托管 MCP 生态尚不成熟。
- pychorus 副歌定位是启发式，非典型结构会错（已知时间戳优先）。
- pip 供应链目前只做到版本锁定 + TLS，hash pinning 是 follow-up。

---

## 许可证

暂缺 LICENSE 文件（默认保留所有权利）。公开发布前请补（建议 MIT 或 Apache-2.0）。
