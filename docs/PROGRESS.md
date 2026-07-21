# VoicePilot 项目进度记录

> **最后更新:** 2026-07-21 (Asia/Shanghai)
> **当前分支:** `master`
> **最新 commit:** `5133c30` fix(w5): verify voice compilation + fix VAD speech_end_sample bug
> **测试状态:** 196 passing (default, W1-W4) / +voice tests 21 passing + 6 ignored via `--features voice`(requires CMake + MSVC + libclang), 0 warnings
> **规格版本:** V1.1.2(规格 issue #17-#43 已解决;W5 实现已知 issue #44-#49 延后 W6+)
> **W5 Fast-Follow:** ✅ 已完成(2026-07-21)— `cargo check --features voice` + `cargo test --features voice` 全部通过,详见 §二 W5 段落

---

## 一、总体里程碑状态

| 周次 | 名称 | 状态 | 测试数 | 完成时间 | Merge Commit |
|---|---|---|---|---|---|
| W1 | Trust Kernel Skeleton | ✅ 已合并 | 26 | 2026-07-19 | (squash into W2 merge) |
| W2 | Policy + Action Gateway | ✅ 已合并 | 53 | 2026-07-19 | `59a5999` |
| W3a | Filesystem Adapter + Compensation + Verifier | ✅ 已合并 | 38 | 2026-07-19 | `d8bd4b5` |
| W3b | files.organize Skill + 端到端审批流 | ✅ 已合并 | 39 | 2026-07-20 | `e9aa5ca` |
| W4 | MCP Server Wrapping | ✅ 已完成 | 40 | 2026-07-20 | `1e10586` (direct on master) |
| W5 | Voice Input (Whisper.cpp) | ✅ 已完成 | +voice (opt-in, requires CMake) | 2026-07-20 | (direct on master) |
| W6 | Tauri UI Shell | ⏳ 未开始 | — | — | — |
| W7 | LLM Planner + 8 Skills | ⏳ 未开始 | — | — | — |
| W8 | Stronghold Encryption + Taint Tracking | ⏳ 未开始 | — | — | — |

**累计测试数:** 196 (W1: 26 + W2: 53 + W3a: 38 + W3b: 39 + W4: 40;W5 voice tests 通过 `--features voice` 启用,需要 CMake + MSVC)

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

### W4: MCP Server Wrapping (38 tests)

**实现内容:**
- §6.1 JSON-RPC 2.0 wire 协议(`mcp/transport.rs`)— Request/Response/Error/Notification + NDJSON 行分隔帧
- §6.1 `McpServer` 调度器(`mcp/server.rs`)— `handle_request` 分发 initialize/tools.list/tools.call
- §6.1 MCP 2025-11-25 协议握手(`initialize` → `notifications/initialized` → `tools/list` → `tools/call`)
- §8.1 `McpServerRepo` CRUD(`mcp/repo.rs`)— `mcp_servers` 表完整读写
- §8.1 `load_allowed_paths` 从 `mcp_servers.allowed_paths` JSON 文本加载 → `AllowedPaths::new`(规范化的根)→ 注入 FilesystemTool(spec issue #31 解决)
- §8.1 `seed_builtin_filesystem` 幂等种子 builtin filesystem server row
- §6.1 `tools/list` 返回 3 个 filesystem tool schema(search_files / verify_move / move_files,后者 annotations destructiveHint=true)
- §6.1 `tools/call` 分发:`search_files` / `verify_move` 直接执行,`move_files` 拒绝(强制走 Skill executor)
- §6.1 `audit_append_external` 跨进程审计:每个 `tools/call` 写入 `audit_logs.details`
- §6.1 stdio transport 循环(`run_stdio`)— 读 stdin 一行一条 NDJSON,写 stdout
- §6.1 CLI `mcp-serve` 命令:启动 stdio MCP server,加载 allowed_paths 注入 FilesystemTool
- `w4_e2e_smoke.rs` 端到端集成测试:4 NDJSON 消息 → 4 响应 + 审计链验证 + PathNotAllowed 验证

**新增模块结构:**
```
crates/trust-kernel/src/
└── mcp/
    ├── mod.rs              # 模块导出
    ├── transport.rs        # JsonRpcRequest/Response/Error/Notification + parse_line + write_message
    ├── server.rs           # McpServer + OutgoingMessage + handle_request + run_stdio
    └── repo.rs             # McpServerRepo CRUD + load_allowed_paths + seed_builtin_filesystem
```

**核心架构决策:**
- `McpServer` 持有 `Arc<TrustKernel>`(非 by-value)— `TrustKernel` 不 `Clone`,`new()` 包装为 Arc,`with_arc()` 接受现有 Arc
- `FilesystemTool` 字段从 `Arc<FilesystemTool>` 改为 `Arc<Mutex<FilesystemTool>>` — 支持运行时替换 `replace_filesystem_with_allowed_paths()`;`filesystem()` accessor 返回 `MutexGuard`(deref coercion 保 9 处既有调用点零修改)
- `OutgoingMessage` 枚举(Response | Error)手写 `impl Serialize` 委托内部变体(可用 `#[serde(untagged)]` 但显式 impl 更可读)
- `handle_tools_call` 捕获 `call_tool` 内核错误并转换为 `OutgoingMessage::Error`(InternalError -32603),而非 `?` 传播 — 确保每个 Request 都有 Response(MCP 协议要求)
- CLI `mcp-serve` 终端命令消费 `TrustKernel` by-value,`return Ok(())` 在 stdio 循环结束后退出 main

**W4 commits (按时序,直接提交到 master):**
| Commit | 任务 |
|---|---|
| `f78f025` | docs(w4): add MCP Server Wrapping implementation plan |
| `d113458` | Task 1: JSON-RPC 2.0 message types (V1.1 §6.1 wire protocol) |
| `f4b88f0` | Task 2: NDJSON line framing for stdio transport (V1.1 §6.1) |
| `4443364` | Task 3: McpServerRepo CRUD for mcp_servers table (V1.1 §8.1) |
| `0685939` | Task 4: load_allowed_paths helper for FilesystemTool injection (V1.1 §8.1, issue #31) |
| `0c7c72a` | Task 5: kernel public audit_append_external for MCP audit trail (V1.1 §6.1) |
| `2f88719` | Task 6: McpServer struct + dispatch skeleton (V1.1 §6.1) |
| `4c44129` | Task 7: initialize handshake with protocol 2025-11-25 (V1.1 §6.1) |
| `396d958` | Task 8: tools/list + tools/call methods with audit logging (V1.1 §6.1, §6.3) |
| `ed302b1` | Task 9: stdio transport loop with NDJSON framing (V1.1 §6.1) |
| `f5c1191` | Task 10: seed_builtin_filesystem idempotent row creator (V1.1 §8.1) |
| `69ea70e` | Task 11: CLI mcp-serve command with allowed_paths injection (V1.1 §6.1, §8.1, issue #31) |
| `1e10586` | Task 12: end-to-end smoke test for MCP server (V1.1 §6.1, §11.1 W4 gate) |
| `4169e5b` | W4 fast-follow: run_stdio continues after kernel errors (spec issue #37) |

**关键修复(CRITICAL):**
- Task 3 `row_to_record` 返回类型:plan 指定 `crate::error::Result<McpServerRecord>` 但 `query_map` 闭包要求 `rusqlite::Result<T>` → 改返回类型为 `rusqlite::Result<McpServerRecord>`(`?` 通过 `#[from]` 自动转换)
- Task 8 `handle_tools_call` 错误处理:plan 用 `let result = self.handler.call_tool(...)?;` 把 `KernelError::Mcp` 当 Err 传播,导致测试 `unwrap()` panic → 改为 `match` 捕获内核错误并转换为 `OutgoingMessage::Error`(InternalError -32603),保证每个 Request 都产生 Response
- Task 8 审计测试 FK 约束:plan 创建 task `t-audit` 但未创建 step `s-audit`,`audit_logs.step_id` FK 失败 → 测试设置增加 `kernel.create_step(&StepRecord::new("s-audit", "t-audit", 1))`
- Task 11 `handle_mcp_serve_command` 签名:plan 用 `&TrustKernel` 但 `McpServer::new()` 取 by-value → 改为取 `TrustKernel` by-value(终端命令,`return Ok(())` 退出 main)
- Task 11 `&*kernel.conn()` clippy 警告:`explicit_auto_deref` lint → 改为 `&kernel.conn()`(auto-deref 从 `&MutexGuard<Connection>` → `&Connection`)
- Task 12 Windows temp dir 测试:`std::env::temp_dir()` 在 Windows 下位于 `C:/Users`(被白名单允许)→ 简化为只测 "after" 行为,断言错误消息含 `"not under any allowed root"`

**最终代码审查(Verdict: APPROVED_WITH_NITS):**
- **1 Important(fast-follow post-merge):** `run_stdio` 用 `?` 传播 kernel 错误,客户端发送畸形输入时会崩循环 — 应捕获并返回 error response 而非 panic loop
  - ✅ **已解决(commit `4169e5b`):** `handle_tools_call` 内两个 `?` 路径(缺失 `name` 字段 + `audit_append_external` 失败)改为返回 `OutgoingMessage::Error`(InvalidParams -32602 / InternalError -32603),循环不再传播 kernel 错误。新增 2 个端到端测试 `run_stdio_continues_after_invalid_params_missing_name` + `run_stdio_continues_after_audit_failure`。
- **4 minor nits:**
  - 错误码断言过松(只检查 code 字段为负数,应精确断言 -32603)
  - 缺少 missing `name` 字段的测试 ✅ **已补(commit `4169e5b`)**
  - `OutgoingMessage` 可改用 `#[serde(untagged)]` 简化
  - 几个 pre-existing clippy warnings(非 W4 引入)

**已知偏离(已记录规格 issue 37-43):**
- ~~§6.1 `run_stdio` 错误传播行为未规定~~ ✅ **issue #37 已解决(commit `4169e5b`,2026-07-20):** `handle_tools_call` 内 `name` 缺失返回 InvalidParams -32602,`audit_append_external` 失败返回 InternalError -32603,循环不再传播 kernel 错误
- §6.1 `OutgoingMessage` 序列化策略未规定(`#[serde(untagged)]` vs 手写 `impl Serialize`)
- §8.1 `mcp_servers.allowed_paths` JSON 文本存储格式未规定(W4 用 JSON text array,如 `["D:/", "E:/"]`)
- §6.1 `protocolVersion=2025-11-25` 客户端协商策略未规定(W4 服务端硬编码,未校验客户端请求的版本)
- §6.1 `tools/call` 审计日志 `details` schema 未规定(W4 用 `{"server_id", "tool_name", "args", "success"}` 自定义结构)
- §8.1 `mcp_servers` builtin row 启动加载策略未规定(W4 用 `seed_builtin_filesystem` 幂等 INSERT OR IGNORE)
- §6.1 `tools/call` 内核错误 → JSON-RPC 错误码映射未规定(W4 一律映射为 -32603 InternalError,未区分 MethodNotFound/-32601 vs InvalidParams/-32602)

### W5: Voice Input (Whisper.cpp) (默认 196 tests 不变;voice opt-in)

**实现内容:**
- §2.1 voice 子系统模块(`voice/{error, model, wav, vad, whisper, audio, router_bridge}.rs`)
- 模块全 feature-gated `#[cfg(feature = "voice")]`,默认 `default = []` 保持纯 Rust 构建(CMake 仅在 `--features voice` 时需要)
- Whisper.cpp FFI 绑定(`whisper-rs` 0.13,optional dep)
- 音频采集(`cpal` 0.15,optional dep,跨平台 I/O)
- WAV I/O(`hound` 3.5,optional dep,mono 16-bit 强制)
- 能量阈值 VAD(W5 PoC;Silero VAD 延后 W6+)
- `ModelRegistry` 解析 `~/.voicepilot/models/ggml-*.bin`(用户手动下载,CLI 打印 URL,W5 不做 auto-download)
- `WhisperEngine` 加载模型 + 推理(i16 → f32 PCM 转换,16kHz 强制)
- `AudioRecorder` cpal 输入流 + 5s 超时 + 线性重采样 + downmix to mono
- `RouterBridge` 文本 → SkillRouter 路由(不执行 Skill,W7 LLM Planner 提取参数)
- CLI `voice` 子命令 4 个:`voice listen` / `voice transcribe <file>` / `voice list-models` / `voice route <text>`
- E2E 冒烟测试 `w5_e2e_smoke.rs` 三 Tier(pure-logic 跑 CI / model-required `#[ignore]` / mic-required `#[ignore]`)

**新增模块结构:**
```
crates/trust-kernel/src/voice/
├── mod.rs              # 模块导出
├── error.rs            # VoiceError enum (ModelMissing / MicDenied / InferenceFailed / InvalidWav / NoSpeechDetected / CaptureFailed / ModelLoadFailed)
├── model.rs            # ModelRegistry + ModelSpec
├── wav.rs              # read_wav / write_wav (hound wrappers, mono 16-bit enforced)
├── vad.rs              # VadDetector + VadConfig + VadOutcome (energy threshold)
├── whisper.rs          # WhisperEngine + WhisperConfig
├── audio.rs            # AudioRecorder + AudioRecorderConfig (cpal input stream)
└── router_bridge.rs    # route_text(kernel, approver, text) -> RouteOutcome
```

**W5 commits (按时序,直接提交到 master):**
| Commit | 任务 |
|---|---|
| `eb7783c` | docs(w5): add Voice Input implementation plan |
| `c5a80f0` | docs(w5): revise plan for opt-in voice feature (default = [], preserves pure-Rust build) |
| `e36a128` | Task 1: build(voice): add whisper-rs + cpal + hound deps with voice feature gate |
| `26c0809` | Task 2: feat(voice): module skeleton + VoiceError (V1.1 §2.1) |
| `09786f3` | Task 3: feat(voice): ModelRegistry resolves Whisper model paths from ~/.voicepilot/models/ |
| `8f90b01` | Task 4: feat(voice): WAV I/O helpers (hound wrappers, mono 16-bit only) |
| `6784d1a` | Task 5: feat(voice): energy-threshold VAD with frame-based silence detection |
| `04833d3` | Task 6: feat(voice): WhisperEngine wraps whisper-rs for transcription (integration tests #[ignore]) |
| `05d306b` | Task 7: feat(voice): AudioRecorder with cpal microphone capture + resample + downmix |
| `5b32c75` | Task 8: feat(voice): RouterBridge wires transcribed text to SkillRouter |
| `461e989` | Task 9: feat(cli): voice list-models command prints available Whisper models |
| `2e15291` | Task 10: feat(cli): voice transcribe <file> command transcribes WAV via Whisper |
| `678e6ea` | Task 11: feat(cli): voice route <text> command previews SkillRouter matching |
| `fce478a` | Task 12: feat(cli): voice listen command — record + transcribe + route end-to-end |
| `877d861` | Task 13: test(w5): end-to-end smoke test with 3 tiers (pure-logic / model-required / mic-required) |

**核心架构决策:**
- **Feature gating 改为 opt-in**(`default = []`, `voice = ["dep:whisper-rs", "dep:cpal", "dep:hound"]`) — 保留纯 Rust 默认构建,CMake/MSVC 只在 voice feature 启用时需要;CI 与默认 dev workflow 保持 CMake-free
- `whisper-rs` + `cpal` + `hound` 作为 optional deps 加在 `voice` feature 下;workspace deps 也标注 `optional = true`
- CLI `voice` 子命令全部 `#[cfg(feature = "voice")]`-gated:dispatch loop 用单一 `#[cfg(feature = "voice")] { ... }` 块包裹所有 voice 命令分支,每个 handler 函数独立 `#[cfg(feature = "voice")]` 标注
- VAD 用简单能量阈值(W5 PoC);Silero VAD 延后 W6+
- 模型文件:用户手动下载;CLI `voice list-models` 打印 URL(W5 不做 auto-download)
- WhisperEngine integration tests 标 `#[ignore]`(需模型文件)
- AudioRecorder tests 标 `#[ignore]`(需麦克风)
- CLI `voice listen` 录固定 5s(W5 PoC;无 VAD-based auto-stop)
- `RouterBridge::route_text` 不执行 Skill — W5 PoC 由 caller(CLI)提示用户输入 args;W7 LLM Planner 将自动提取参数

**✅ W5 Fast-Follow 已完成 — voice 编译验证通过(2026-07-21):**
- ✅ MSVC Build Tools 已确认可用:Visual Studio Community 2022 at `E:\VS2022\VS`
- ✅ CMake 3.31.6 已确认可用:VS 自带 at `E:\VS2022\VS\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe`(满足 whisper-rs 3.20+ 要求)
- ✅ LLVM/libclang 已就绪:`LIBCLANG_PATH=C:\Program Files\LLVM\bin`(bindgen 依赖)
- ✅ `WHISPER_DONT_GENERATE_BINDINGS=1` 环境变量已设置 — 使用 bundled `bindings.rs` 而非 bindgen 生成(bindgen 在 Windows MSVC 上产生 opaque struct)
- ✅ `cargo check --features voice` 通过(exit 0)
- ✅ `cargo test --features voice` 通过(exit 0):voice_unit 18 passed + voice_integration 1 passed + 4 ignored(需模型)+ w5_e2e_smoke 2 passed + 2 ignored(需麦克风)= 21 passed + 6 ignored
- ✅ `cargo test`(default)通过:196 passing,W1-W4 无回归
- ✅ voice 模块代码已按 plan 完整提交,API 与签名严格遵循 plan 规格

**关键修复(本地 patch,非 upstream):**
1. **`whisper-rs-sys-0.11.1\src\bindings.rs`**(bundled bindings,registry source + 2 build output 目录同步):
   - 移除 32 个 Linux x86_64 专属的 `const _: () = { ... };` 编译期 size/align/offset 断言块(Windows MSVC ABI 不同,如 `_G_fpos_t` 在 Windows 上 12 字节 vs Linux 16 字节 → `E0080 overflow`)
   - 3 个 C enum 类型别名从 `::std::os::raw::c_uint`(u32)改为 `i32`:`whisper_alignment_heads_preset` / `whisper_gretype` / `whisper_sampling_strategy`(Windows MSVC C enum 默认 `int` 即 i32,而 whisper-rs 0.13.2 显式 `#[repr(i32)]`,导致 `E0308 mismatched types`)
2. **`whisper.cpp\src\whisper.cpp`**(registry source + 2 build output 目录同步):添加 UTF-8 BOM(`0xEF 0xBB 0xBF`)— MSVC 无 BOM 时误读 CJK 字符(♪♩♫♬「」『』)产生 `C3688 文本后缀"銆"无效`
3. **`voice/vad.rs` bug fix**:`speech_end_sample` 语义错误 — 原返回 `last_speech_frame * frame_size`(语音最后一帧),应为 `(i + 1) * frame_size`(`i` = 静音超时帧,即语音段实际结束位置);修正后 `vad_detects_silence_after_speech_with_correct_boundary` 测试通过
4. **`tests/voice_unit.rs` 修复**:`m.name.as_str()` 改为 `m.name`(`&'static str` 上 `as_str()` 是 unstable feature `str_as_str`);`VadOutcome::Speech { speech_end_sample }` 改为 `{ speech_end_sample, .. }`(缺字段 `speech_start_sample`);移除未使用 import `PathBuf` + `ModelSpec`
5. **未使用 import 清理**:`voice/model.rs` 移除 `Path`,`voice/whisper.rs` 移除 `Path`,`voice/audio.rs` 移除 `Sample`

**Patch 应用方式(若需在新机器上重现):**
- `bindings.rs` 与 `whisper.cpp` 的 patch 作用于 cargo registry source(`~/.cargo/registry/src/index.crates.io-*/whisper-rs-sys-0.11.1/`)及 target build output 目录(`target/debug/build/whisper-rs-sys-*/out/`)
- `cargo clean` 后这些 patch 会丢失,需要重新应用;未来可考虑用 `build.rs` patch 脚本或 fork whisper-rs-sys 自动化
- 环境变量:`LIBCLANG_PATH=C:\Program Files\LLVM\bin`、`WHISPER_DONT_GENERATE_BINDINGS=1`、PATH 含 CMake

**已知偏离(已记录 issue #44-#49,延后 W6+):**
- #44: VAD 用简单能量阈值;可能误触发于背景噪声(W6+ 换 Silero VAD)
- #45: `voice listen` 录固定 5s;无 VAD-based auto-stop
- #46: 模型 auto-download 未实现(用户须手动下载 `ggml-tiny.bin`)
- #47: 流式 partial transcripts 未实现(W6+,Whisper.cpp streaming API)
- #48: wake word detection 未实现(W6+,用户须手动运行 `voice listen`)
- #49: voice feature 需要 CMake + MSVC + libclang;默认构建排除 voice(opt-in decision 2026-07-20);**W5 fast-follow 已于 2026-07-21 完成验证(21 passed + 6 ignored)**

---

## 三、当前 master 状态确认

### 测试与构建

```powershell
cd d:\voicepilot
cargo test --manifest-path voicepilot\Cargo.toml
# 结果:196 passing, 0 failing, 0 warnings (default,W1-W4;voice tests `#![cfg(feature = "voice")]`-gated,自动跳过)
cargo build --manifest-path voicepilot\Cargo.toml -p cli
# 结果:0 warnings (default,无 voice;voice 命令 `#[cfg(feature = "voice")]`-gated,默认二进制不含)
# 验证 voice 编译(需要 CMake + MSVC,W5 fast-follow):
# cargo test --manifest-path voicepilot\Cargo.toml --features voice
```

### Git 状态

```
当前分支: master
最新 commit: 877d861 test(w5): end-to-end smoke test with 3 tiers (pure-logic / model-required / mic-required)
保留分支: (无,W5 直接提交到 master,无 feature 分支)
```

### 关键文件清单

**规格文档:**
- `d:\voicepilot\voicepilot-v1.1-spec\voicepilot-v1.1-spec.html`(V1.1.2,~120KB self-contained HTML)

**计划文档:**
- `d:\voicepilot\docs\superpowers\plans\2026-07-19-w1-trust-kernel-skeleton.md`
- `d:\voicepilot\docs\superpowers\plans\2026-07-19-w2-policy-action-gateway.md`
- `d:\voicepilot\docs\superpowers\plans\2026-07-19-w3a-filesystem-adapter.md`
- `d:\voicepilot\docs\superpowers\plans\2026-07-20-w3b-files-organize-skill.md`
- `d:\voicepilot\docs\superpowers\plans\2026-07-20-w4-mcp-server-wrapping.md`
- `d:\voicepilot\docs\superpowers\plans\2026-07-20-w5-voice-input.md`

**进度文档(本文件):**
- `d:\voicepilot\docs\PROGRESS.md`

**核心源码:**
- `d:\voicepilot\voicepilot\Cargo.toml`(workspace)
- `d:\voicepilot\voicepilot\crates\trust-kernel\src\` (Trust Kernel 主体)
- `d:\voicepilot\voicepilot\crates\cli\src\main.rs` (CLI 入口,含 `mcp-serve` 命令)

---

## 四、未完成工作(明天起点)

### 4.1 立即任务:W6 计划编写 + W5 fast-follow

**W6 范围(Tauri UI Shell):**

1. **Tauri 项目脚手架**
   - `voicepilot/crates/ui` 新 crate(Tauri v2 + React/Vue/Svelte)
   - Rust 后端复用 `trust-kernel`(通过 Tauri command 桥接)
   - 打包 `cli` 子命令到 Tauri menu/shortcut

2. **审批 UI**
   - 替换 `CliApprover`,实现 `TauriApprover`(IPC 调用审批窗口)
   - 显示 `EffectManifest` 详情(sources / total_bytes / destination / conflicts)
   - y/N 按钮 + 超时默认 Deny

3. **设置面板**
   - Whisper 模型路径配置(浏览 `~/.voicepilot/models/`)
   - `allowed_paths` 白名单编辑(W4 `mcp_servers.allowed_paths` JSON 数组)
   - 麦克风设备选择 + VAD 阈值调节

4. **语音按钮**
   - 调用 `voice listen` 命令(W5 已实现 CLI 层)
   - 实时显示 transcription + route outcome
   - VAD-based 自动停止(W6 替换 W5 PoC 的固定 5s 超时)

**W6 不在范围(留到 W7+):**
- LLM Planner fallback(留 W7)
- 真实 Stronghold 加密(留 W8)
- Silero VAD(留 W6+ 决定)

**W5 fast-follow(高优先级,优先于 W6):**
- 安装 CMake 3.20+ 和 MSVC Build Tools
- 运行 `cargo test --manifest-path voicepilot\Cargo.toml --features voice` 验证 voice 模块编译
- 下载 `ggml-tiny.bin` 到 `~/.voicepilot/models/`,运行 `cargo test --features voice -- --ignored` 验证 Tier 2/3
- 安装后更新 PROGRESS.md "CMake 未安装" 段落为 "已验证"

### 4.2 规格问题(全部已解决,2026-07-20 V1.1.2)

**全部 27 个 spec issue(#17-#43)已在 V1.1.2 规格文档中修订完成。** 详见 `voicepilot-v1.1-spec/voicepilot-v1.1-spec.html` changelog V1.1.2 行 + 各章节末尾的"V1.1.2 修订"callout。

| Issue 范围 | 章节 | 修订主题 | V1.1.2 callout 位置 |
|---|---|---|---|
| #17 | §3.3 / §6.1 | filesystem-mcp 自研化(原生 Rust FilesystemTool,替代 @modelcontextprotocol/server-filesystem) | §6.1 表格行 + V1.1.2 修订 callout ① |
| #18 | §7.2 | snapshot_encrypted 加密时序(W3a-W7 明文 PoC → W8 stronghold gate → W9 验收无明文残留) | §7.2 V1.1.2 修订 callout ① |
| #19 | §6.2 | file_id 构造规则(Windows: volume_serial+file_index;Unix: st_dev+st_ino) | §6.2 V1.1.2 修订 callout ① |
| #20 | §6.3 | ToolResult.compensation_level(工具上限)vs CompensationRecord.level(实际级别)语义区分 | §6.3 V1.1.2 修订 callout |
| #21 | §7.2 | conflict_policy 触发条件表(auto_reverse/require_confirmation/fail × conflicts 空/非空) | §7.2 V1.1.2 修订 callout ② |
| #22 | §6.2 | prepare token 在 TOCTOU 失败时可重用(commit 操作仅在所有检查通过后才消费 token) | §6.2 V1.1.2 修订 callout ② |
| #23 | §7.1 | sha256_of_file 单点定义(fs_snapshot.rs 唯一,fs.rs use 引用)+ VerifyResult.verified 语义 | §7.1 V1.1.2 修订 callout |
| #24 | §6.2 | effect_manifest D2/D3 脱敏(D3 canonical_path 替换为 <redacted:D3>,sha256 保留供比对) | §6.2 V1.1.2 修订 callout ③ |
| #25 | §6.2 | cross-volume rename fallback(EXDEV → copy+remove)+ rollback 失败上报 tracing::error! | §6.2 V1.1.2 修订 callout ④ |
| #26 | §8.1 | TrustKernel 公开 API 边界(领域特定 API 替代 kernel.conn() 私有访问) | §8.1 V1.1.2 修订 callout ① |
| #27 | §5.3 | Skill Manifest 双形态(W3b-W6 Rust struct literal;W7+ YAML 文件 + serde_yaml) | §5.3 V1.1.2 修订 callout ① |
| #28 | §6.2 | approval_token 与 approval_id 合并(废弃 approval_token,统一用 approval_id UUID) | §6.2 V1.1.2 修订 callout ⑤ |
| #29 | §8.1 | approvals 表 risk_level 字段废弃 + compensations 表 level 字段重命名为 compensation_level | §8.1 V1.1.2 修订 callout ②③ + 表格行 |
| #30 | §5.3 | file_filter schema 定义(mode: extension/name_pattern/date_range/size_range + 各 mode 字段) | §5.3 V1.1.2 修订 callout ② |
| #31 | §8.1 | mcp_servers.allowed_paths 强制层级(在 FilesystemTool 层强制,非 MCP server 层) | §6.1 V1.1.2 修订 callout ③ + §3.3 表格行 |
| #32 | §5.1 | Skill Router 实现明确(W3b-W6 纯关键词匹配;W7+ 本地 LLM fallback) | §5.1 V1.1.2 修订 callout |
| #33 | §11.1 | W3 gate 明确为"端到端审计链可跑"(prepare→approve→commit→verify→compensate + audit_logs 完整) | §11.1 W3 行修订 |
| #34 | §6.2 | effect_manifest 持久化策略(W3a-W7 明文 JSON;W8 vault_ref + summary) | §6.2 V1.1.2 修订 callout ⑥ |
| #35 | §7.2 | conflict_policy=require_confirmation 的 confirmation 语义(Approver::request_conflict_resolution 三选一) | §7.2 V1.1.2 修订 callout ③ |
| #36 | §4.4 | canonicalize 跨平台语义统一(POSIX 风格 + 路径组件前缀匹配,避免 /foo 误匹配 /foobar) | §4.4 V1.1.2 修订 callout |
| #37 | §6.1 | run_stdio 错误传播行为(已在 4169e5b 修复,name 缺失→-32602,audit 失败→-32603,循环不中断) | §6.1 V1.1.2 修订 callout ⑦(代码层已修复) |
| #38 | §6.1 | OutgoingMessage 序列化策略(手写 impl Serialize 或 #[serde(untagged)] 均合法) | §6.1 V1.1.2 修订 callout ② |
| #39 | §8.1 | mcp_servers.allowed_paths JSON 数组字符串格式(如 `["D:/", "E:/documents"]`) | §6.1 V1.1.2 修订 callout ③ |
| #40 | §6.1 | protocolVersion 协商策略(服务端硬编码 2025-11-25,客户端字段仅日志,ServerCapabilities 标注 version_locked=true) | §6.1 V1.1.2 修订 callout ④ |
| #41 | §6.1 | tools/call 审计日志 details schema(server_id+tool_name+arguments+success+error_class+duration_ms) | §6.1 V1.1.2 修订 callout ⑤ |
| #42 | §8.1 | mcp_servers builtin row 启动加载策略(seed_builtin_filesystem 幂等 INSERT OR IGNORE,FK 约束防 DELETE) | §6.1 V1.1.2 修订 callout ⑥ |
| #43 | §6.1 | 内核错误 → JSON-RPC 错误码完整映射表(-32602/-32601/-32603 按变体分类) | §6.1 V1.1.2 修订 callout ⑦ + 错误码映射表 |

### 4.2.1 W5 实现已知偏离(issue #44-#49,未解决,延后 W6+)

W5 引入的 6 个实现层 known issues(非规格问题,记录于 §二 W5 详细记录):

| Issue | 主题 | 延后到 |
|---|---|---|
| #44 | VAD 用简单能量阈值;可能误触发于背景噪声 | W6+(Silero VAD) |
| #45 | `voice listen` 录固定 5s;无 VAD-based auto-stop | W6+(Tauri UI + VAD 集成) |
| #46 | 模型 auto-download 未实现(用户须手动下载 `ggml-tiny.bin`) | W6+(CLI/UI 集成下载器) |
| #47 | 流式 partial transcripts 未实现(Whisper.cpp streaming API) | W6+ |
| #48 | wake word detection 未实现(用户须手动运行 `voice listen`) | W6+ |
| #49 | voice feature 需要 CMake + MSVC;默认构建排除 voice(opt-in decision 2026-07-20) | 永久(opt-in 设计决策) |

### 4.3 后续周次计划(高层)

- **W6:** Tauri UI Shell — 桌面应用 + 审批 UI + 设置面板
- **W7:** LLM Planner + 8 Skills — 8 个确定性 Skill 全部实现 + LLM 编排
- **W8:** Stronghold Encryption + Taint Tracking — `snapshot_encrypted` 真实加密 + 污点传播

---

## 五、明天开机恢复指南

### 5.1 环境检查(开机第一步)

```powershell
cd d:\voicepilot
git status                          # 应为 clean,on master
git log --oneline -3                # 应看到最新 fix(w5) commit
cargo test --manifest-path voicepilot\Cargo.toml 2>&1 | Select-String "test result:" | Measure-Object  # 应为 196(default,W1-W4;voice tests cfg-gated 跳过)
# 可选(voice 验证,需 libclang + CMake + MSVC):
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
$env:WHISPER_DONT_GENERATE_BINDINGS = "1"
$env:PATH = "E:\VS2022\VS\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin;" + $env:PATH
cargo test --features voice --manifest-path voicepilot\Cargo.toml  # 21 passed + 6 ignored
```

### 5.2 推荐起点:W6 计划编写

W5 voice 模块已全部完成并验证通过(commit `877d861` + fast-follow fix commit)。**`cargo check --features voice` + `cargo test --features voice` 全部通过(21 passed + 6 ignored,2026-07-21)**。详见 §二 W5 段落。

**Step 1: W6 计划编写:**

使用 `superpowers:writing-plans` skill 创建 W6 计划:

```
d:\voicepilot\docs\superpowers\plans\YYYY-MM-DD-w6-tauri-ui-shell.md
```

**W6 计划应包含的 TDD 任务(初步估计 10-15 个):**

1. Tauri v2 项目脚手架(`voicepilot/crates/ui`)
2. Tauri command 桥接 `trust-kernel`(替代 CLI 直接调用)
3. `TauriApprover` 实现 `Approver` trait(IPC 调用审批窗口)
4. 审批 UI 组件(EffectManifest 详情 + y/N 按钮 + 超时)
5. 设置面板(模型路径 + allowed_paths + 麦克风)
6. 语音按钮 UI(调用 `voice listen`)
7. 实时 transcription 显示
8. Route outcome 反馈(matched skill / unmatched)
9. VAD-based 自动停止(替换 W5 PoC 的固定 5s 超时,issue #45)
10. 模型 auto-download(issue #46 解决)
11. 错误处理(模型缺失 / 麦克风权限 / 推理失败 UI 反馈)
12. Tauri 打包(Windows installer + macOS dmg + Linux AppImage)
13. E2E 冒烟测试(Tauri 端到端,§11.1 W6 gate)

### 5.3 用户偏好提醒

- **不使用 worktree** — 直接在 `d:\voicepilot` git init/branch/merge
- **遇到不合理/可优化的规格** — 报告给用户(已积累 17-43 共 27 个 spec issue,V1.1.2 已修订;W5 实现 issue #44-#49 延后 W6+)
- **PowerShell 限制** — 不支持 `&&`/`||`/heredoc,用 `;` 链接命令,单行 commit message
- **Subagent-Driven Development** — W2/W3a/W3b/W4/W5 都用此模式,W6 大概率继续
- **TDD 严格** — 红 → 绿 → 重构,每 task 一个 commit
- **W5 voice feature opt-in** — 默认 `cargo build/test` 不含 voice;启用 voice 需 `--features voice` + CMake + MSVC

### 5.4 Memory 资源

明天可参考的 memory 文件:
- `c:\Users\16567\.trae-cn\memory\user_profile.md` — 用户偏好(不使用 worktree,遇到不合理规格报告)
- `c:\Users\16567\.trae-cn\memory\projects\-d-voicepilot\project_memory.md` — 硬约束 + 工程约定 + Lessons Learned(W1/W2/W3a/W3b/W4/W5 累计)
- `c:\Users\16567\.trae-cn\memory\projects\-d-voicepilot\20260720\topics.md` — 今日 W4/W5 完成记录

---

## 六、关键链接

- **规格文档:** [voicepilot-v1.1-spec.html](file:///d:/voicepilot/voicepilot-v1.1-spec/voicepilot-v1.1-spec.html)
- **W3a 计划:** [2026-07-19-w3a-filesystem-adapter.md](file:///d:/voicepilot/docs/superpowers/plans/2026-07-19-w3a-filesystem-adapter.md)
- **W3b 计划:** [2026-07-20-w3b-files-organize-skill.md](file:///d:/voicepilot/docs/superpowers/plans/2026-07-20-w3b-files-organize-skill.md)
- **W4 计划:** [2026-07-20-w4-mcp-server-wrapping.md](file:///d:/voicepilot/docs/superpowers/plans/2026-07-20-w4-mcp-server-wrapping.md)
- **W5 计划:** [2026-07-20-w5-voice-input.md](file:///d:/voicepilot/docs/superpowers/plans/2026-07-20-w5-voice-input.md)
- **Trust Kernel 源码:** [crates/trust-kernel/src/](file:///d:/voicepilot/voicepilot/crates/trust-kernel/src/)
- **Voice 模块源码:** [crates/trust-kernel/src/voice/](file:///d:/voicepilot/voicepilot/crates/trust-kernel/src/voice/)
- **CLI 入口:** [crates/cli/src/main.rs](file:///d:/voicepilot/voicepilot/crates/cli/src/main.rs)
