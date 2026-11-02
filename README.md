# VoicePilot

![CI](https://github.com/Suxiao-yong/VoicePilot/actions/workflows/ci.yml/badge.svg)

> VoicePilot is a permission-aware Windows voice agent built on one Rust trust kernel. Speak or type one sentence — download a song, clip its chorus, organize files, launch apps — every side effect passes risk assessment, user approval and hash-chained audit. The cloud LLM only understands and plans; execution always goes through the local kernel. **The same kernel now ships in two forms: the Tauri desktop app (IPC) and a headless HTTP/SSE gateway (`voicepilot serve`).**

VoicePilot 是一个**权限感知**的 Windows 语音 Agent。说一句话（或打一行字），搜歌、下载、截副歌、整理文件、开关应用一气呵成——每个副作用背后都有风险评估、审批确认与审计留痕。云端 LLM 只负责理解与规划，**执行永远走本地信任内核**。

- **一套内核，两种形态**：桌面走 Tauri IPC；无头模式 `voicepilot serve` 暴露 5 个 HTTP/SSE 接口，共享同一个 SQLite、同一份 Cedar 策略包、同一条审计哈希链。
- **高频指令零 token**：确定性快路由正则一步出结果，实测 P95 13.9 ms，比 LLM 通道快约 88 倍。
- **高危才弹审批**：改/删文件、`shell.run`、不可逆提交；其余 50+ 技能免审批直行、全量审计。
- **外部 MCP 也过策略网关**：注解 → 风险等级映射 → 污点硬拒 → Cedar + E×D 矩阵，`Confirm/Deny` 一律 fail-closed。
- **可撤销、可观测**：UI 一键 Undo 回滚已执行步骤；审计页顶部给出分通道 P50/P95、缓存命中率与审计链完整性。

---

## 目录

- [功能详解](#功能详解)
- [HTTP/SSE 网关服务](#httpsse-网关服务)
- [架构](#架构)
- [Skill 清单](#skill-清单)
- [技术栈](#技术栈)
- [快速开始](#快速开始)
- [构建矩阵](#构建矩阵)
- [配置说明](#配置说明)
- [使用指南](#使用指南)
- [测试](#测试)
- [性能基准（实测）](#性能基准实测)
- [目录结构](#目录结构)
- [安全模型](#安全模型)
- [文档索引](#文档索引)
- [已知限制与路线图](#已知限制与路线图)
- [许可证](#许可证)

---

## 功能详解

### 一句话办事（三种入口 + 一个 API）

主窗口输入框、麦克风语音（VAD 自动断句 + Push-to-talk 全局热键）、桌宠气泡，以及无头模式的 `POST /api/v1/route`——同一套路由与执行。

```text
整理下载目录          → files.organize（整理归档，一批一次确认）
下载晴天              → media.download（歌名搜源下载，声明假设版本）
截这首歌的副歌        → media.clip_chorus（副歌定位，截 60 秒）
打开记事本            → quick.app_control（应用控制）
每天早上八点总结      → task.schedule（后台定时作业）
搜到多个版本时        → 追问卡三选一（超时回默认）
```

### 路由：三级策略、六个判定点

1. **站点直达**：`打开抖音搜索世界杯` → 站内 URL 直出（`sys.open_url`）。
2. **原子快路由**（`fastroute`）：正则一步完成意图匹配 + 参数提取，零 LLM 开销；形状不对一律回退，不挤掉正常流量。
3. **关键词路由**：Skill 自带的 keywords / intent_examples 确定性命中。
4. **LLM classify**：高置信命中进 SQLite 缓存 24h，复述零 HTTP。
5. **LLM DAG 拆解**：复杂任务拆成多节点计划（拓扑执行 + 骨架审批 + 失败归因）。
6. **聊天兜底**：都不命中时直接回答，不执行、无副作用。

每次规划都会写一条 `route_kind` 延迟样本（`fastroute` / `keyword` / `llm` / `cache`），供可观测面板与基准脚本消费。

### 记忆（两层）

- **情景记忆**：每轮封轮一条摘要（`turns` 表，原文音频永不落盘，30 天 TTL，隐私模式跳过）；下一轮快照自动带上文，LLM 可回指“还是刚才那个”。
- **长期记忆**：会话结束从用户发言蒸馏 ≤5 条事实（偏好/约定/纠正），相似合并、矛盾软删不自动裁决；召回 top-3 注入并声明来源。`memory.view` / `memory.forget` 给用户否决权。

### 一键撤销（Undo）

- 后端：`compensations` 表 + `ReverseFnRegistry`（5 个逆向函数：`filesystem.reverse_move` / `note.reverse_capture` / `research.reverse_save` / `form.reverse_prepare` / `media.reverse_clip_chorus`）+ `task.compensate` 全管道（定位 active 记录 → 反向执行 → 强校验 → 标记 `reversed`）。
- 前端：执行结果卡与审计页的「可补偿动作」列表渲染 **一键撤销** 按钮；点击后按钮原地变为徽章 **已补偿回滚**，失败展示后端错误原文。
- 只有**存在 `compensations` 记录**的步骤可撤；记录 TTL 过期后不再出现在列表里。

### 延迟 / 缓存可观测

- 审计页顶部 4 张指标卡：快路由、关键词、LLM 三条通道各自的 **P50 / P95**，缓存命中率，以及**审计哈希链完整性**（逐行重算 SHA-256）。
- 数据源：`voice_latency_samples`（migration 013 新增 `route_kind` / `cache_hit` 两列）+ `audit_logs` 里 `llm_decompose_called` 的 `token_count`。
- 同一份聚合也通过 `GET /api/v1/observability` 暴露，桌面与无头模式口径一致。

### 后台作业

进程内调度器秒级 tick 扫 `agent_jobs` 表，到期走同一套 planner 执行，结果进任务历史 + 桌面通知。语义写死：**进程不在期间的漏跑不追补**；DAG 内每步照走审批白名单，用户不在屏默认 Deny。

### 媒体链路（下载副歌一分钟）

歌名 → `ytsearch` 找源下载（top-3 标题消歧，明确胜出直下）→ pychorus 定位副歌 → 截取 → `ffmpeg -i` 验时长（±5s）。已知时间戳优先走 `trim_video`；pychorus 只是启发式 fallback，判错可能如实声明。

---

## HTTP/SSE 网关服务

桌面壳之外的第二形态：**没有 Tauri、没有 WebView**，一个二进制跑在服务器/容器里，仍然走同一套信任内核。

```bash
voicepilot serve [--host 127.0.0.1] [--port 8080] [--db voicepilot.db]
# → voicepilot serve listening on http://127.0.0.1:8080
```

| Method | Path | 说明 |
| --- | --- | --- |
| POST | `/api/v1/route` | 路由判定（6 个判定点）+ Slot 提取，返回 `{kind, skill_id, slots, dag, route_kind, latency_ms, used_llm}` |
| POST | `/api/v1/execute` | 执行技能；需 `idempotency_key`，可选 `prepare_token` + `manifest`（TOCTOU 预检） |
| POST | `/api/v1/compensate` | 触发 `task.compensate` 事务级逆向补偿 |
| GET | `/api/v1/events` | **SSE**（`text/event-stream`）推送 `route` / `execute` / `compensate` 事件，15s 心跳 |
| GET | `/api/v1/observability` | 分通道 P50/P95、缓存命中率、DAG token 聚合、审计链完整性 |

```bash
curl -s localhost:8080/api/v1/route -H 'content-type: application/json' \
  -d '{"text":"帮我填这个表单"}'
# {"kind":"skill","skill_id":"form.prepare","route_kind":"keyword","latency_ms":3,...}

curl -sN localhost:8080/api/v1/events      # SSE 流
curl -s localhost:8080/api/v1/observability | jq .cache
```

**执行语义（安全相关，别跳过）**

- **幂等**：`idempotency_key` 命中直接返回首次结果并置 `idempotent_replay: true`；同 key 并发第二个请求 409（`in_flight` 占坑），不会双执行。
- **TOCTOU**：`prepare_token` 必须伴随 `manifest`；校验顺序 = token 存在 → 未过期（5 min TTL）→ `preconditions_hash` 重算一致。任一失败 409 且**不执行**；`validate` 只读，`commit` 才消费 token。
- **无审批者 = fail-closed**：`fs.write_file|fs.create_file|fs.delete_file|shell.run` 在 HTTP 侧一律拒绝，其余技能用 `AutoDenier`（任何 executor 内部 `approver.prompt` 都拿到 Deny）。需要人工确认的能力只能在桌面端走。
- **无鉴权**：当前网关没有认证，只适合本机/内网；公网部署前必须加网关鉴权（见路线图）。

容器化：仓库内 `voicepilot/Dockerfile`（两阶段 `rust:1.85-bookworm` → `debian:bookworm-slim`）。

```bash
docker build -t voicepilot:latest voicepilot
docker run --rm -p 8080:8080 -v voicepilot-data:/data voicepilot:latest
```

---

## 架构

```text
用户（语音 / 文字 / 桌宠 / HTTP 客户端）
  │
  ├─ Tauri 2 Shell（主窗口 + 桌宠 + 托盘）/ React 前端
  │    IPC（invoke + event；WebView 不直访文件系统与 MCP）
  │
  └─ voicepilot serve（Axum）：5 个 HTTP/SSE 接口
       │  route / execute / compensate / events / observability
       ▼
信任内核 Trust Kernel（Rust，单一信任边界，两种入口共用）
  ├─ 感知：VAD → 转写 → RealtimeSnapshot（10s 新鲜度门禁）→ 记忆召回
  ├─ 规划：站点直达 → 快路由 → 关键词 → LLM classify → LLM 拆 DAG → 聊天兜底
  ├─ 决策：Cedar 策略 + E×D 风险矩阵 + 审批 + 追问卡（MCP 也走这条链）
  ├─ 执行：Skill 分发 / MCP 客户端 / UIA 自动化
  └─ 善后：强校验 → 补偿 → 哈希链审计 → 污染溯源
        ▲ 云端 LLM（只理解规划）   ▲ Stronghold 加密   ▲ SQLite 落库
```

**一句话走完全程的例子**（“下载晴天”）：

1. `parse_download` 命中 → `{url: ytsearch1:晴天}`（零 token）。
2. executor 拉 top-3 标题，唯一命中直下，否则追问卡。
3. yt-dlp 下载（缺席自动供给 + SHA 校验），输出声明“默认下载《…》”。
4. 落 task/step + 审计；可补偿（删文件），UI 出现「一键撤销」。

**核心不变量**：LLM、前端、外部 MCP 都在信任壳之外——只能提议，不能执行；未知 skill_id 在快照门直接拒绝，执行器进程都起不来；任何不确定都失败闭合（fail-closed），返回可读错误。

---

## Skill 清单

| 组 | Skill（* 需对应 feature/外部程序） |
| --- | --- |
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
| --- | --- |
| 内核 | Rust（tokio、rusqlite、serde）、Cedar 策略引擎、SQLite |
| 桌面 | Tauri 2（tray-icon、global-shortcut、dialog）、WebView2 |
| 前端 | React 18 + TypeScript + Vite + Vitest + Testing Library |
| 服务端 | Axum 0.8 + tower-http（TraceLayer）+ tokio-stream（broadcast → SSE） |
| 语音 | sherpa-rs 预编译包（ASR/TTS/VAD，无需 CMake） |
| 外部集成 | Playwright MCP、Composio MCP（邮件试点）、mcp-windows（UIA 后端） |
| 评测 | Inspect AI、promptfoo、TOCTOU/红队/恶意 Server 场景集（`evals/`） |
| 工程 | GitHub Actions（CI/audit/docs/release）、cargo-deny、rustdoc + TypeDoc、changeset、Docker |

架构决策见 [`docs/adr/`](docs/adr/)（11 篇：平台、LLM 策略、加密、策略引擎、MCP 版本、语音、UI、DAG、污染追踪、评测、供应链）。

---

## 快速开始

### 前置要求

- Rust stable（`rustup`）、Node.js 18+、Windows 10+（WebView2 一般自带；安装包缺席会自动引导下载）。
- 语音/模型与 yt-dlp/ffmpeg 首次使用时自动供给，需要联网；pychorus 走 pip（`==` 锁定版本）。
- 磁盘：构建缓存较大（`target/` 可达百 GB 级，`.gitignore` 已覆盖；`cargo clean` 可随时清掉重编）。

### 桌面应用

```powershell
cd voicepilot

# 全功能桌面应用（推荐）：语音 + LLM + UIA + 内嵌前端
cargo build --release -p voicepilot-ui --features voice,custom-protocol,uia
.\target\release\voicepilot-ui.exe
```

> ⚠️ `custom-protocol` 必开：Tauri 2 按该特性（而非 debug/release）决定窗口加载内嵌前端还是 dev 服务器。不开则双击 exe 显示 `localhost 拒绝连接`。

首次启动：进 **Settings** 填云端 LLM API Key（保存即生效，重启自动重建；密钥进系统凭据库）；语音模型在 ModelDownloadBar 下载。配置与数据落在 `%APPDATA%\voicepilot`，不进仓库。

### 无头网关

```bash
cd voicepilot
cargo build --release -p cli
./target/release/voicepilot serve --host 127.0.0.1 --port 8080 --db ./voicepilot.db

# 可选：注入云端 LLM（三者齐全才启用；缺任一则退化为快路由 + 关键词）
export VOICEPILOT_LLM_BASE_URL=https://api.deepseek.com/v1
export VOICEPILOT_LLM_API_KEY=sk-...
export VOICEPILOT_LLM_MODEL=deepseek-chat
```

---

## 构建矩阵

| 命令 | 说明 |
| --- | --- |
| `cargo build -p trust-kernel --no-default-features` | 纯内核逻辑（无 LLM/语音/UIA），CI 最快门 |
| `cargo build -p trust-kernel --features voice,llm` | 内核 + 语音 + LLM（路由/DAG 全功能） |
| `cargo build -p trust-kernel --features uia` | + Windows 应用控制 |
| `cargo build -p trust-kernel --features stronghold` | + 加密快照 |
| `cargo build -p cli` | `voicepilot` 二进制（`serve` / `eval` / 语音子命令；默认含 llm） |
| `cargo build -p voicepilot-ui --features voice,custom-protocol,uia` | 全功能桌面 exe（默认已含 llm） |
| `cargo build -p voicepilot-ui --features tauri` | 仅 UI 壳（无语音，编译最快） |

前端单独构建：`cd voicepilot/crates/ui/web && npm install && npm run build`（`web/dist/index.html` 已提交，Tauri 编译期内嵌需要它）。

---

## 配置说明

| 项 | 位置/方式 |
| --- | --- |
| LLM Key、开关 | 应用内 Settings 页（持久化 KV + 系统凭据库）；无头模式用 `VOICEPILOT_LLM_{BASE_URL,API_KEY,MODEL}` |
| 数据库路径 | 默认运行目录 `voicepilot.db`，可用 `VOICEPILOT_DB` 覆盖；`serve --db` 优先于环境变量 |
| 监听地址 | `serve --host` / `--port`（`--port 0` 由内核分配，启动行打印真实端口） |
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
- **查账**：Audit Viewer 按任务查哈希链 + 顶部可观测指标卡；`task.explain` 问失败归因。
- **回滚**：执行结果卡 / 审计页点「一键撤销」→ 徽章变「已补偿回滚」。
- **对外集成**：`voicepilot serve` + `POST /api/v1/route`，或订阅 `/api/v1/events` 做实时看板。

---

## 测试

```powershell
cd voicepilot
chcp 65001   # 中文断言需 UTF-8 代码页，否则 shell/剪贴板用例乱码（环境问题非回归）

cargo test -p trust-kernel --lib
cargo test -p trust-kernel --features uia --lib
cargo test --workspace --no-fail-fast -j 2   # 链接器省内存

cargo clippy --workspace --all-targets -- -D warnings

cd crates/ui/web && npm test && npm run build
```

**当前实测**（本机 Windows / rustc 1.97.1）：

| 项 | 数值 |
| --- | --- |
| Rust 测试属性 | 1,359（1,262 `#[test]` + 97 `#[tokio::test]`，含 feature-gated） |
| 一次 `cargo test --workspace` | **1,036 例：1,032 通过 / 0 失败 / 4 ignored**（ignored 为 live-LLM / Playwright canary） |
| 前端 Vitest | 11 个文件 / 60 个用例（实跑 61 例：60 通过 / 1 skipped） |
| clippy | `--workspace --all-targets -- -D warnings` 零告警 |

测试分层：纯函数单测 → python-mock MCP 全链路 → wiremock 挡云端 → 真机冒烟（麦克风/真 OAuth 留给手工清单，见各功能节）。

---

## 性能基准（实测）

`evals/benchmarks/bench_route_latency.py`：启动真实 `voicepilot serve --port 0`，四组各 20 句式 × 5 轮（首轮丢弃），逐条断言 `route_kind`；跑完直读 SQLite `voice_latency_samples` 交叉校验。

| 通道 | 样本 | P50 | P95 | 相对 LLM |
|---|---:|---:|---:|---|
| fastroute（本地正则） | 80 | **3.19 ms** | **13.93 ms** | ≈380× / 88× |
| keyword（关键词） | 80 | **3.39 ms** | **14.23 ms** | ≈357× |
| cache（路由缓存命中） | 100 | **3.22 ms** | **13.58 ms** | ≈376× |
| llm（**本地 stub 固定 1.2s**） | 80 | **1209.89 ms** | **1229.60 ms** | 基准 |

- 快路由通道是**双峰分布**（P50 在两遍完整跑里落在 3.19 / 13.10 ms 两档，机器负载敏感）；**P95 稳定在 13.9–15.0 ms**，对外引用优先用 P95。
- LLM 组数字来自本地 OpenAI 兼容 stub，**不是真实云端 SLA**；换真 key 后需重跑脚本。
- 安全场景（`summarize_scenarios.py`，真实执行 205 条）：攻击类 **105/105 = 100%** 拦截（红队 50 + TOCTOU 20 + 恶意 MCP 15 + 数据安全 20）。
- 复现：`python evals/benchmarks/bench_route_latency.py`（产物写 `evals/reports/bench-<ts>.json`，reports 不入库）。

---

## 目录结构

```text
voicepilot/                        # Rust workspace
  crates/
    trust-kernel/                  # 信任内核
      src/
        approval/                  # Approver trait + 审批类型（含追问语义分离）
        policy/                    # Cedar 引擎 + E×D 矩阵 + 事务 effect + taint
        gateway.rs                 # 决策网关：硬拒 → Cedar → 约束 → E×D → egress
        skills/                    # 按域拆分：sys/clip/fs/web/shell/media/pim/doc
        │                         # + fastroute / dispatcher / dag_executor
        │                         # + manifest / router / reverse_fns（补偿）
        mcp/                       # JSON-RPC 传输 + Server + 客户端 + policy（MCP 策略映射）
        voice/                     # VAD/录音/转写/模型供给/路由桥
        uiautomation/              # UIA 适配（外部 mcp-windows 后端）
        compensation/              # 补偿类型 + ReverseFnRegistry
        memory.rs scheduler.rs secrets.rs external_scan.rs  # 记忆/作业/密钥/扫描
        turns.rs llm_cache.rs planner.rs voice_latency.rs  # 记忆/缓存/规划/延迟样本
      migrations/                  # 001–013（013 = route_kind + cache_hit）
      tests/                       # 集成冒烟（dag/红队/toctou/恶意server/数据安全/MCP 策略…）
    ui/                            # Tauri 应用：commands / undo / observability / approver / pet
      web/src/                     # React：MainView / AuditViewerView / UndoButton / 信任中心…
    cli/                           # voicepilot 二进制：serve（HTTP/SSE）/ eval / 语音子命令
  Dockerfile                       # 无头网关镜像（两阶段构建）
docs/
  adr/                             # 11 篇架构决策记录
  PROGRESS.md                      # 里程碑进度记录
  release.md                       # 发版说明
evals/
  functional/ redteam/ toctou/ malicious_server/ data_security/   # 205 场景
  benchmarks/                      # 延迟 + 场景汇总脚本（纯标准库）
tools/mcp-windows/                 # UIA 后端（二进制，本地下载，不入仓）
sherpa-libs/                       # 语音运行时（二进制，本地下载，不入仓）
```

---

## 安全模型

- **执行白名单**：未知/禁用/仅展示 skill 在快照门直接拒绝，执行器与 MCP 进程跑不起来。
- **MCP 全链路（本轮补齐）**：`verify_mcp_annotations`（谎报只读检测）→ 参数 taint 硬拒（`web_page → ToolArgument`）→ `Gateway::decide`（Cedar + E×D + egress）→ `mcp_policy_decision` 审计；注解到风险等级的映射是纯函数，单测锁定。
  - 映射：`destructiveHint→E2` / `openWorldHint→E3` / `readOnlyHint→E0` / 默认 E1；参数携带凭据形态 → D3（红线硬拒）。
  - `Confirm` / `Deny` 一律 `PolicyDenied`：MCP 路径没有审批通道，fail-closed。
- **MCP 纵深**：transport 白名单（stdio）+ 协议版本锁定 `2025-11-25` + 工具 schema 哈希基线 + 60s/4MB 调用限额 + 污染溯源（`mcp_tool:<server_id>` 进审计）。
- **密钥卫生**：外部凭据只存 `$keyring:` 引用；值永不进 renderer、不进日志；缺失 fail-closed。
- **隐私模式**：记忆零写入、LLM 零注入，降级为关键词路由。
- **补偿与追溯**：删文件/清表单类操作登记 reverse 函数；审计哈希链防篡改（`verify_chain` 可随时校验）；记忆注入声明来源。
- **HTTP 边界**：无审批者 → 审批白名单技能直接拒绝、其余 executor 用 `AutoDenier`；幂等键防重复副作用；`PrepareToken` 防 TOCTOU；**无鉴权**，只适合本机/内网。

---

## 文档索引

- [`docs/adr/`](docs/adr/)：架构决策（背景 / 方案 / 否决项 / 后果）。
- [`docs/PROGRESS.md`](docs/PROGRESS.md)：里程碑与测试统计。
- [`docs/release.md`](docs/release.md)：发版流程（NSIS + WebView2 bootstrapper）。
- [`voicepilot/docs/`](voicepilot/docs/)：Skill 与扩展开发说明、MCP 插件示例。

---

## 已知限制与路线图

- **HTTP 网关无鉴权**：只适合本机/内网；公网部署需先加认证与限流（路线图第一位）。
- **幂等表与 SSE 扇出是进程内状态**：多副本需换 Redis/DB，重启即失效。
- **无头模式不能执行需人工审批的技能**：`fs.write_file|fs.create_file|fs.delete_file|shell.run` 一律拒绝；远程审批通道尚未实现。
- 调度器是进程内的：退出期间的漏跑不追补；要 OS 级可靠再上 schtasks（v1 不做）。
- 记忆蒸馏依赖 LLM 质量，记错可用 `memory.forget` 删；矛盾事实不自动裁决。
- Composio 邮件链路依赖第三方云；备选自托管 MCP 生态尚不成熟。
- pychorus 副歌定位是启发式，非典型结构会错（已知时间戳优先）。
- pip 供应链目前只做到版本锁定 + TLS，hash pinning 是 follow-up。
- 基准里的 LLM 延迟来自本地 stub，不是真实云端 SLA。

---

## 许可证

暂缺 LICENSE 文件（默认保留所有权利）。公开发布前请补（建议 MIT 或 Apache-2.0）。
