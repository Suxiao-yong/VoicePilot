# VoicePilot 项目进度记录

> **最后更新:** 2026-07-20 (Asia/Shanghai)
> **当前分支:** `master`
> **最新 commit:** `e9aa5ca` Merge W3b
> **测试状态:** 156 passing, 0 warnings
> **规格版本:** V1.1.1 (W3b 新增 issue 27-35 待 V1.1.2 修订)

---

## 一、总体里程碑状态

| 周次 | 名称 | 状态 | 测试数 | 完成时间 | Merge Commit |
|---|---|---|---|---|---|
| W1 | Trust Kernel Skeleton | ✅ 已合并 | 26 | 2026-07-19 | (squash into W2 merge) |
| W2 | Policy + Action Gateway | ✅ 已合并 | 53 | 2026-07-19 | `59a5999` |
| W3a | Filesystem Adapter + Compensation + Verifier | ✅ 已合并 | 38 | 2026-07-19 | `d8bd4b5` |
| W3b | files.organize Skill + 端到端审批流 | ✅ 已合并 | 39 | 2026-07-20 | `e9aa5ca` |
| W4 | MCP Server Wrapping | ⏳ 未开始 | — | — | — |
| W5 | Voice Input (Whisper.cpp) | ⏳ 未开始 | — | — | — |
| W6 | Tauri UI Shell | ⏳ 未开始 | — | — | — |
| W7 | LLM Planner + 8 Skills | ⏳ 未开始 | — | — | — |
| W8 | Stronghold Encryption + Taint Tracking | ⏳ 未开始 | — | — | — |

**累计测试数:** 156 (W1: 26 + W2: 53 + W3a: 38 + W3b: 39)

---

## 二、已完成工作详细记录

### W1: Trust Kernel Skeleton (26 tests)

**实现内容:**
- `crates/trust-kernel/src/` 单一 Rust 信任内核骨架
- SQLite 数据库迁移(`db/mod.rs`)
- `TaskRepo` + `StepRepo` CRUD(级联删除)
- `TaskState` 状态机(覆盖所有合法/非法转换)
- `SqliteAuditLogger` 哈希链审计日志
- `TrustKernel` facade(`open_in_memory` / `open_file`)
- `cli` crate(text command entry: create/cancel/show/quit)

**关键修复:**
- Rust lifetime:`query_map` 闭包不能引用 `&Row<'_>`,改为提取为 owned 类型
- CLI EOF:无 `if n == 0 { break; }` 导致死循环
- Audit test FK 失败:`task_id` 引用必须先创建 parent task

### W2: Policy + Action Gateway (53 tests)

**实现内容:**
- §4.1 E×D 二维风险矩阵(`policy/risk_matrix.rs`)
- §4.2 Cedar 授权引擎 + Rust Constraint Engine 双层决策
- `ActionGateway` 编排 Cedar + Constraint + E×D + Egress
- `policy` CLI 命令(E×D 决策演示)
- V1.1.1 规格修订(W2 反馈)

**关键偏离:**
- Cedar 4.11.2 API 调整(`EntityUid::from_type_name` 等)
- Token 过期检查修复

### W3a: Filesystem Tool Adapter + Compensation + Strong Verifier (38 tests)

**实现内容:**
- §4.4 step 1 `fs_paths::canonicalize` 路径规范化(纯字符串,不触碰文件系统)
- §6.1 `FilesystemTool` 原生 Rust 适配器(无 MCP SDK 依赖)
- §6.2 `prepare_move` + `commit_move` + TOCTOU 防护(`preconditions_hash`)
- §6.3 `ToolResult` V2 完整字段(status + evidence + compensation + idempotency + data_classification)
- §7.1 Strong Verifier(`verify_move` 重读 sha256+size)
- §7.2 三级 Compensation(`CompensationLevel::None/BestEffort/Strong`)
- §7.2 `auto_reverse_move` 自动回滚
- §8.1 `compensations` 表 CRUD
- CLI `move` 命令端到端冒烟(prepare → commit → verify → compensation placeholder)

**新增模块结构:**
```
crates/trust-kernel/src/
├── tools/
│   ├── mod.rs
│   ├── fs.rs              # FilesystemTool
│   ├── fs_paths.rs        # canonicalize
│   └── fs_snapshot.rs     # snapshot_file (sha256 + size + mtime + file_id)
├── compensation/
│   ├── mod.rs
│   ├── types.rs           # CompensationLevel, ConflictPolicy, CompensationRecord
│   ├── repo.rs            # CompensationRepo CRUD
│   └── executor.rs        # auto_reverse_move
└── toolresult.rs          # ToolResult V2
```

**W3a commits (按时序):**
| Commit | 任务 |
|---|---|
| `2bf7b5c` | Task 1: walkdir dep + path canonicalization |
| `2e1b3bc` | Task 2: file snapshot helpers |
| `c482bd5` | Task 3: FilesystemTool::prepare_move |
| `95c5a3f` | Task 4: FilesystemTool::commit_move |
| `02b0b28` | Task 5: Strong Verifier + ToolResult V2 skeleton |
| `26c7dea` | Task 6: search_files with glob filter |
| `e461fc0` | Task 7: Compensation types + repo |
| `1ceffda` | Task 8: auto_reverse_move |
| `aefce5f` | Task 9: ToolResult V2 + kernel accessors + CLI move |
| `1e1aa4e` | chore: drop unused W2 imports |
| `d8bd4b5` | Merge W3a (--no-ff) |

**关键修复(CRITICAL):**
- Task 8 rollback rename 方向反转:plan 的 `rename(curr, orig)` 是反的,改为 `rename(orig, curr)`

**已知偏离(已记录规格 issue 17-26):**
- §6.1 计划用 Node `@modelcontextprotocol/server-filesystem`,但 V1.1 要求单一 Rust 内核 → W3a 改为原生 Rust
- §7.2 `snapshot_encrypted` 须用 tauri-plugin-stronghold,但 W3a-W7 用明文 PoC,W8 才上 Stronghold
- §6.2 `file_id` 构造未规定,Windows 用 `volume_serial + file_index`,Unix 用 `dev + ino`
- §6.3 `ToolResult.compensation_level` 与 `CompensationRecord.level` 语义重叠(前者是工具上限,后者是实际级别)
- §7.2 `conflict_policy` 触发条件未定义
- §6.2 prepare token 在 new-conflict TOCTOU check 之前被消费,caller 无法用原 token 重试
- §7.1 `sha256_of_file` 在 fs.rs 和 fs_snapshot.rs 重复;`VerifyResult.verified` 字段冗余
- §8.1 PoC stash hack:当提供真实 vault_ref 时 `compensate_fn`/`reverse_payload` 丢失
- §7.2 cross-volume rename 未处理;rollback 失败静默吞没
- §6.3 CLI `move` 命令 Phase 4 补偿记录创建被推迟到 W3b(`kernel.conn` 私有)

### W3b: files.organize Skill + 端到端审批流 (39 tests)

**实现内容:**
- §5.1 `SkillRouter` 纯关键词匹配路由(intent_examples + curated keywords,无 LLM)
- §5.2/§5.3 `SkillManifest` 完整 schema + `files.organize` 内置 manifest(E2/D2/local_only/4 steps)
- §6.2 prepare → approve → commit → verify → compensate 全链路 `FilesOrganizeSkill` 编排器
- §6.2 `Approver` trait(UI 无关)+ `CliApprover` 交互式审批提示 + `AutoApprover` 测试用
- §8.1 `ApprovalRepo` CRUD + `approvals` 表持久化 + `approval_scope=Single`
- §6.1 `McpHandler` 骨架 + `McpToolSchema`(inputSchema/outputSchema/annotations per Appendix B)
- §6.1 `McpHandler::call_tool("filesystem.move_files", ...)` 返回 Err(强制走 Skill 执行器)
- §4.4/§8.1 `AllowedPaths` 白名单 + `FilesystemTool::new_with_allowed_paths()` 强制
- §8.1 `kernel.create_compensation` / `record_approval` / step lifecycle 公开方法(替换 W3a 占位)
- CLI `organize <root> <filter> <dest>` 命令端到端冒烟(§11.1 W3 gate)
- `w3b_e2e_smoke.rs` 完整端到端集成测试(含 auto_reverse_move 往返)

**新增模块结构:**
```
crates/trust-kernel/src/
├── skills/
│   ├── mod.rs
│   ├── manifest.rs         # SkillManifest + files_organize_manifest
│   ├── router.rs           # SkillRouter + RouteDecision
│   └── executor.rs         # FilesOrganizeSkill 编排器
├── approval/
│   ├── mod.rs
│   ├── types.rs            # ApprovalDecision, ApprovalScope, ApprovalRecord
│   ├── repo.rs             # ApprovalRepo CRUD
│   └── approver.rs         # Approver trait + AutoApprover
├── mcp/
│   ├── mod.rs
│   ├── schema.rs           # McpToolSchema, McpAnnotations (camelCase)
│   └── handler.rs          # McpHandler::list_tools + call_tool
└── allowed_paths.rs        # AllowedPaths 白名单
```

**W3b commits (按时序):**
| Commit | 任务 |
|---|---|
| `09554af` | Task 1: kernel compensation + step 公开 accessors |
| `15f66a9` | Task 2: approval types + repo + Approver trait |
| `0fa1eb3` | Task 2 fix: DRY row parsing + ELevel as_str/parse |
| `0867315` | Task 3: kernel.record_approval + audit chain |
| `209bfb6` | Task 4: SkillManifest schema + files.organize builtin |
| `1286447` | Task 4 fix: remove unused imports + unqualify HashMap |
| `e61b131` | Task 5: SkillRouter with intent keyword matching |
| `502395e` | Task 5 fix: curated keywords field (false-positive fix) |
| `6f4fe52` | Task 6: AllowedPaths whitelist enforcement |
| `5447d45` | Task 6 fix: prefix-ancestor test + PathNotAllowed assertions |
| `717e00a` | Task 7: files.organize executor (prepare-approve-commit-verify-compensate) |
| `a32b537` | Task 7 fix: error-path correctness (mark Failed on commit/verify/comp failure) |
| `2e5f6dc` | Task 8: MCP handler skeleton |
| `79d6cdc` | Task 8 fix: lowercase evidence_strength + camelCase serde + test rename |
| `8becf62` | Task 9: CLI organize command + CliApprover |
| `561c71d` | Task 9 fix: clippy print_literal nit |
| `17b2b55` | Task 10: end-to-end smoke test (§11.1 W3 gate) |
| `833cc78` | final review fix: verify-failure 路径补偿创建错误显式上报 |
| `e9aa5ca` | Merge W3b (--no-ff) |

**关键修复(CRITICAL):**
- Task 7 错误路径正确性:`commit_move` / `verify_move` / `create_compensation` 任一失败时必须先 `update_step_status(Failed)` 再传播错误
- Task 7 verify-fail 后必须合成 CompensationRecord(否则已 commit 的 move 无法回滚)
- Task 7 `create_compensation` 失败后必须显式上报错误(否则 commit 已落盘但无补偿记录 → 数据丢失)
- Task 8 `format!("{:?}", evidence_strength)` 产生 `"Strong"`(大写)违反 `#[serde(rename_all = "lowercase")]` 约定 → 添加 `EvidenceStrength::as_str()` 方法
- Task 8 `McpToolSchema` / `McpAnnotations` 字段需 `#[serde(rename_all = "camelCase")]` 以匹配 W4 JSON-RPC 序列化

**已知偏离(已记录规格 issue 27-35):**
- §5.3 Skill Manifest 用 YAML 但 V1.1 技术栈未指定 `serde_yaml` → W3b 用 Rust struct literal,W7 加 YAML 支持
- §6.2 `approval_token` 提及但格式未定义 → 与 `approval_id` 语义重叠,建议合并
- §8.1 `approvals` 表同时有 `risk_level`(V1.0 legacy)和 `E_level`/`D_level` → 冗余,建议 V1.2 弃用
- §5.3 `file_filter` 类型 schema 未定义 → W3b 当作 glob 字符串
- §8.1 `mcp_servers.allowed_paths` 强制层级未规定 → W3b 在 FilesystemTool 层强制(更灵活)
- §5.1 Skill Router "轻量模型" 未指定具体模型 → W3b 用纯关键词匹配(确定性)
- §11.1 W3 gate "可跑" 范围模糊 → W3b 实现严格版(完整审计链)
- §6.2 `effect_manifest` D2/D3 数据脱敏未规定
- §7.2 `conflict_policy=require_confirmation` 的 "confirmation" 语义未定义
- §4.4 `fs_paths::canonicalize` 在 Unix 上剥离前导 `/`(issue #36,W3b 已在 `allowed_paths.rs` 文档注释)

---

## 三、当前 master 状态确认

### 测试与构建

```powershell
cd d:\voicepilot
cargo test --manifest-path voicepilot\Cargo.toml
# 结果:156 passing, 0 failing, 0 warnings
cargo build --manifest-path voicepilot\Cargo.toml -p cli
# 结果:0 warnings
```

### Git 状态

```
当前分支: master
最新 commit: e9aa5ca Merge W3b: files.organize Skill + End-to-End Approval Flow
保留分支: (无,W3b feature 分支已删除)
```

### 关键文件清单

**规格文档:**
- `d:\voicepilot\voicepilot-v1.1-spec\voicepilot-v1.1-spec.html`(V1.1.1,~120KB self-contained HTML)

**计划文档:**
- `d:\voicepilot\docs\superpowers\plans\2026-07-19-w1-trust-kernel-skeleton.md`
- `d:\voicepilot\docs\superpowers\plans\2026-07-19-w2-policy-action-gateway.md`
- `d:\voicepilot\docs\superpowers\plans\2026-07-19-w3a-filesystem-adapter.md`

**进度文档(本文件):**
- `d:\voicepilot\docs\PROGRESS.md`

**核心源码:**
- `d:\voicepilot\voicepilot\Cargo.toml`(workspace)
- `d:\voicepilot\voicepilot\crates\trust-kernel\src\` (Trust Kernel 主体)
- `d:\voicepilot\voicepilot\crates\cli\src\main.rs` (CLI 入口)

---

## 四、未完成工作(明天起点)

### 4.1 立即任务:W4 计划编写

**W4 范围(根据 W3b §"Gaps deferred to W4+"):**

1. **真实 MCP JSON-RPC transport(stdio/SSE)**
   - W3b 已交付 `McpHandler::list_tools()` / `call_tool()` 调度骨架
   - W4 需包一层 JSON-RPC 2.0 wire 协议(stdio + SSE)
   - 对接外部 MCP client(Claude Desktop / Cursor 等)

2. **`mcp_servers` 表持久化 + `allowed_paths` 配置加载**
   - §8.1 `mcp_servers.allowed_paths` 当前未在 kernel 层强制
   - W4 从 `mcp_servers` 表加载 → 注入 `FilesystemTool::new_with_allowed_paths()`
   - 解决 W3b spec issue #31

3. **MCP tool 粒度拆分**
   - W3b `move_files` 整体拒绝(强制走 Skill executor)
   - W4 拆分:`filesystem.search_files` / `filesystem.verify_move` 直接暴露
   - `filesystem.move_files` 仍需走 Skill 审批(或暴露 prepare/commit 两步)

4. **跨进程审计 + SkillRouter 暴露为 MCP tool**
   - `skills.list` / `skills.route` 暴露给 MCP client
   - 外部 client 可查询可用 Skill + 路由建议

5. **集成测试:外部 MCP client → W3b Skill 端到端**
   - 模拟 MCP client 调用 `filesystem.move_files`,验证被拒绝
   - 模拟 MCP client 调用 `files.organize` Skill(经 LLM Planner 或直接)

**W4 不在范围(留到 W5+):**
- Voice Input(留 W5)
- Tauri UI(留 W6)
- LLM Planner fallback(留 W7)
- 真实 Stronghold 加密(留 W8)

### 4.2 待修订规格问题(留待 V1.1.2)

**W3a 计划文档已记录的 planning-time issue(17-21):**

| # | 章节 | 问题 | 建议 |
|---|---|---|---|
| 17 | §6.1 | "filesystem-mcp uses `@modelcontextprotocol/server-filesystem`" 与单一 Rust 内核冲突 | host 原生 Rust,MCP 是薄包装 |
| 18 | §7.2 ① | `snapshot_encrypted` 须用 stronghold,但 W3a-W7 需明文 PoC | 加注:W3a-W7 明文,W8 gate |
| 19 | §6.2 | `file_id: "fi_001"` 构造未定义 | pin per-OS file_id 构造 |
| 20 | §6.3 | `ToolResult.compensation_level` 与 `CompensationRecord.level` 语义重复 | 区分:工具上限 vs 实际级别 |
| 21 | §7.2 | `conflict_policy` 触发条件未定义 | 加 (level, policy) → behavior 表 |

**W3a 执行期发现的新 issue(22-26):**

| # | 来源 | 问题 |
|---|---|---|
| 22 | Task 4 | prepare token 在 new-conflict TOCTOU check 之前被消费,caller 无法用原 token 重试 |
| 23 | Task 5 | 测试依赖 Windows NTFS 大小写不敏感;`sha256_of_file` 在 fs.rs 和 fs_snapshot.rs 重复;`VerifyResult.verified` 字段冗余 |
| 24 | Task 7 | PoC stash hack:当提供真实 `vault_ref` 时 `compensate_fn`/`reverse_payload` 丢失 |
| 25 | Task 8 | cross-volume rename 未处理(Windows 跨卷 `std::fs::rename` 失败);rollback 失败静默吞没 |
| 26 | Task 9 | CLI `move` 命令 Phase 4 补偿记录创建被推迟到 W3b(`kernel.conn` 私有) |

**W3b 执行期发现的新 issue(27-36):**

| # | 来源 | 问题 |
|---|---|---|
| 27 | §5.3 | Skill Manifest 用 YAML 但 V1.1 技术栈未指定 `serde_yaml` |
| 28 | §6.2 | `approval_token` 提及但格式未定义,与 `approval_id` 语义重叠 |
| 29 | §8.1 | `approvals` 表同时有 `risk_level`(V1.0 legacy)和 `E_level`/`D_level`,冗余 |
| 30 | §5.3 | `inputs.<param>.type: file_filter` schema 未定义 |
| 31 | §8.1 | `mcp_servers.allowed_paths` 强制层级未规定(本 kernel vs MCP server) |
| 32 | §5.1 | Skill Router "轻量模型" 未指定具体模型,无法验证 §1.4 "命中率 ≥ 40%" |
| 33 | §11.1 | W3 gate "可跑" 范围模糊(端到端审计链 vs 仅跑通) |
| 34 | §6.2 | `effect_manifest` 对 D2/D3 数据脱敏未规定 |
| 35 | §7.2 | `conflict_policy=require_confirmation` 的 "confirmation" 语义未定义 |
| 36 | §4.4 | `fs_paths::canonicalize` 在 Unix 上剥离前导 `/`,导致根 `/foo` 误匹配 `/foobar`(W3b 已在 `allowed_paths.rs` 文档注释) |

### 4.3 后续周次计划(高层)

- **W4:** MCP Server Wrapping — 把 W3b `McpHandler` 包成 JSON-RPC 2.0 server(stdio/SSE),加载 `mcp_servers` 表配置
- **W5:** Voice Input — Whisper.cpp 集成,语音 → 文本 → Skill 调用
- **W6:** Tauri UI Shell — 桌面应用 + 审批 UI + 设置面板
- **W7:** LLM Planner + 8 Skills — 8 个确定性 Skill 全部实现 + LLM 编排
- **W8:** Stronghold Encryption + Taint Tracking — `snapshot_encrypted` 真实加密 + 污点传播

---

## 五、明天开机恢复指南

### 5.1 环境检查(开机第一步)

```powershell
cd d:\voicepilot
git status                          # 应为 clean,on master
git log --oneline -3                # 应看到 e9aa5ca Merge W3b
cargo test --manifest-path voicepilot\Cargo.toml 2>&1 | Select-String "test result:" | Measure-Object  # 应为 156
```

### 5.2 推荐起点:W4 计划编写

使用 `superpowers:writing-plans` skill 创建 W4 计划:

```
d:\voicepilot\docs\superpowers\plans\YYYY-MM-DD-w4-mcp-server-wrapping.md
```

**W4 计划应包含的 TDD 任务(初步估计 8-12 个):**

1. JSON-RPC 2.0 wire 协议解析器(request/response/error notification)
2. stdio transport(读写 stdin/stdout,行分隔 NDJSON)
3. SSE transport(可选,先做 stdio)
4. `mcp_servers` 表 CRUD + 启动时加载 allowed_paths 配置
5. `McpServer` 结构体:把 `McpHandler` 包成完整 MCP server
6. `tools/list` JSON-RPC method 返回 `McpHandler::list_tools()` 结果
7. `tools/call` JSON-RPC method 分发到 `McpHandler::call_tool()`
8. 跨进程审计:每个 MCP 调用记录到 `audit_logs`
9. `skills.list` / `skills.route` MCP tool 暴露
10. 集成测试:模拟 MCP client 调用 `filesystem.move_files` 被拒绝
11. 集成测试:模拟 MCP client 调用 `files.organize` Skill 跑通
12. CLI `mcp-serve` 命令启动 stdio MCP server

### 5.3 用户偏好提醒

- **不使用 worktree** — 直接在 `d:\voicepilot` git init/branch/merge
- **遇到不合理/可优化的规格** — 报告给用户(已积累 17-36 共 20 个,待 V1.1.2 统一处理)
- **PowerShell 限制** — 不支持 `&&`/`||`/heredoc,用 `;` 链接命令,单行 commit message
- **Subagent-Driven Development** — W2/W3a/W3b 都用此模式,W4 大概率继续
- **TDD 严格** — 红 → 绿 → 重构,每 task 一个 commit

### 5.4 Memory 资源

明天可参考的 memory 文件:
- `c:\Users\16567\.trae-cn\memory\user_profile.md` — 用户偏好(不使用 worktree,遇到不合理规格报告)
- `c:\Users\16567\.trae-cn\memory\projects\-d-voicepilot\project_memory.md` — 硬约束 + 工程约定 + Lessons Learned(W1/W2/W3a/W3b 累计)
- `c:\Users\16567\.trae-cn\memory\projects\-d-voicepilot\20260720\topics.md` — 今日 W3b 完成记录

---

## 六、关键链接

- **规格文档:** [voicepilot-v1.1-spec.html](file:///d:/voicepilot/voicepilot-v1.1-spec/voicepilot-v1.1-spec.html)
- **W3a 计划:** [2026-07-19-w3a-filesystem-adapter.md](file:///d:/voicepilot/docs/superpowers/plans/2026-07-19-w3a-filesystem-adapter.md)
- **W3b 计划:** [2026-07-20-w3b-files-organize-skill.md](file:///d:/voicepilot/docs/superpowers/plans/2026-07-20-w3b-files-organize-skill.md)
- **Trust Kernel 源码:** [crates/trust-kernel/src/](file:///d:/voicepilot/voicepilot/crates/trust-kernel/src/)
- **CLI 入口:** [crates/cli/src/main.rs](file:///d:/voicepilot/voicepilot/crates/cli/src/main.rs)
