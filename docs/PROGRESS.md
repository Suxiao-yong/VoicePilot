# VoicePilot 项目进度记录

> **最后更新:** 2026-07-19 23:50 (Asia/Shanghai)
> **当前分支:** `master`
> **最新 commit:** `d8bd4b5` Merge W3a
> **测试状态:** 117 passing, 0 warnings
> **规格版本:** V1.1.1 (10 个 issue 待 V1.1.2 修订)

---

## 一、总体里程碑状态

| 周次 | 名称 | 状态 | 测试数 | 完成时间 | Merge Commit |
|---|---|---|---|---|---|
| W1 | Trust Kernel Skeleton | ✅ 已合并 | 26 | 2026-07-19 | (squash into W2 merge) |
| W2 | Policy + Action Gateway | ✅ 已合并 | 53 | 2026-07-19 | `59a5999` |
| W3a | Filesystem Adapter + Compensation + Verifier | ✅ 已合并 | 38 | 2026-07-19 | `d8bd4b5` |
| W3b | files.organize Skill + 端到端审批流 | ⏳ 未开始 | — | — | — |
| W4 | MCP Server Wrapping | ⏳ 未开始 | — | — | — |
| W5 | Voice Input (Whisper.cpp) | ⏳ 未开始 | — | — | — |
| W6 | Tauri UI Shell | ⏳ 未开始 | — | — | — |
| W7 | LLM Planner + 8 Skills | ⏳ 未开始 | — | — | — |
| W8 | Stronghold Encryption + Taint Tracking | ⏳ 未开始 | — | — | — |

**累计测试数:** 117 (W1: 26 + W2: 53 + W3a: 38)

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

---

## 三、当前 master 状态确认

### 测试与构建

```powershell
cd d:\voicepilot
cargo test --manifest-path voicepilot\Cargo.toml
# 结果:117 passing, 0 failing, 0 warnings
```

### Git 状态

```
当前分支: master
最新 commit: d8bd4b5 Merge W3a: Filesystem Tool Adapter + Compensation + Strong Verifier
保留分支: w3a-filesystem-adapter (未删除,可留作历史)
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

### 4.1 立即任务:W3b 计划编写

**W3b 范围(根据 W3a 计划文档 §"Gaps deferred to W3b"):**

1. **`files.organize` Skill manifest + Skill Router 集成**
   - V1.1 §3.x Skills 层的 8 个确定性 Skill 之一
   - 调用 `FilesystemTool::prepare_move` / `commit_move` / `verify_move`
   - 生成 `ToolResult` V2

2. **Approval UI flow(prepare → 用户 approve → commit)**
   - V1.1 §6.2 三阶段协议的 approve 阶段
   - 当前 W3a CLI 是 prepare → commit 直通,W3b 需插入用户审批
   - 至少 CLI 级别(prompt "approve? y/n"),Tauri UI 留 W6

3. **MCP server wrapping**
   - V1.1 §6.1 inputSchema / outputSchema / annotations
   - 暴露 `files.organize` / `files.search` / `files.move` 等 MCP tool
   - W3a 用原生 Rust,W3b 包一层 MCP server handler

4. **`allowed_paths` whitelist 强制**
   - V1.1 §4.4 资源规范化后的路径白名单
   - caller 责任,直到 MCP 层落地

5. **`kernel.create_compensation(rec)` 公共方法**
   - W3a CLI Phase 4 placeholder 的真实实现
   - 暴露 `kernel.conn` 给 CLI 的替代方案:在 kernel 提供 `create_compensation` 方法
   - 同时暴露 `kernel.step_repo()` 或专门的 `update_step_prepare_state` / `update_step_post_commit` 方法

6. **端到端冒烟测试**
   - Skill 调用 → 准备 → 审批 → 提交 → 验证 → 补偿记录创建
   - 验证整条链路在 SQLite 中留下完整审计轨迹

**W3b 不在范围(留到 W4+):**
- Tauri UI(留 W6)
- LLM Planner(留 W7)
- 真实 Stronghold 加密(留 W8)
- Taint tracking 传播(留 W8)

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

### 4.3 后续周次计划(高层)

- **W4:** MCP Server Wrapping — 把 W3a 原生 Rust `FilesystemTool` 包成 MCP server(inputSchema/outputSchema/annotations),供外部 MCP client 调用
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
git log --oneline -3                # 应看到 d8bd4b5 Merge W3a
cargo test --manifest-path voicepilot\Cargo.toml 2>&1 | Select-String "test result:" | Measure-Object  # 应为 117
```

### 5.2 推荐起点:W3b 计划编写

使用 `superpowers:writing-plans` skill 创建 W3b 计划:

```
d:\voicepilot\docs\superpowers\plans\2026-07-20-w3b-files-organize-skill.md
```

**W3b 计划应包含的 TDD 任务(初步估计 8-10 个):**

1. `kernel.create_compensation(rec)` 公共方法(替换 W3a CLI placeholder)
2. `kernel.update_step_prepare_state` / `update_step_post_commit` 公共方法
3. `files.organize` Skill manifest 定义(YAML 或 Rust struct)
4. Skill Router(根据 user_goal 选择 Skill)
5. Approval prompt(CLI 级别,`approve? y/n`)
6. MCP server handler 骨架(inputSchema/outputSchema)
7. `allowed_paths` whitelist 强制
8. 端到端冒烟测试(Skill 调用 → 审批 → 提交 → 验证 → 补偿记录)
9. (可选)MCP client 测试(用 rmcp 或类似 SDK 调用本地 server)

### 5.3 用户偏好提醒

- **不使用 worktree** — 直接在 `d:\voicepilot` git init/branch/merge
- **遇到不合理/可优化的规格** — 报告给用户(已积累 17-26 共 10 个,待 V1.1.2 统一处理)
- **PowerShell 限制** — 不支持 `&&`/`||`/heredoc,用 `;` 链接命令,单行 commit message
- **Subagent-Driven Development** — W2/W3a 都用此模式,W3b 大概率继续
- **TDD 严格** — 红 → 绿 → 重构,每 task 一个 commit

### 5.4 Memory 资源

明天可参考的 memory 文件:
- `c:\Users\16567\.trae-cn\memory\user_profile.md` — 用户偏好(不使用 worktree,遇到不合理规格报告)
- `c:\Users\16567\.trae-cn\memory\projects\-d-voicepilot\project_memory.md` — 硬约束 + 工程约定 + Lessons Learned(W1/W2/W3a 累计 9 条)
- `c:\Users\16567\.trae-cn\memory\projects\-d-voicepilot\20260719\topics.md` — 今日 W3a 完成记录

---

## 六、关键链接

- **规格文档:** [voicepilot-v1.1-spec.html](file:///d:/voicepilot/voicepilot-v1.1-spec/voicepilot-v1.1-spec.html)
- **W3a 计划:** [2026-07-19-w3a-filesystem-adapter.md](file:///d:/voicepilot/docs/superpowers/plans/2026-07-19-w3a-filesystem-adapter.md)
- **Trust Kernel 源码:** [crates/trust-kernel/src/](file:///d:/voicepilot/voicepilot/crates/trust-kernel/src/)
- **CLI 入口:** [crates/cli/src/main.rs](file:///d:/voicepilot/voicepilot/crates/cli/src/main.rs)
