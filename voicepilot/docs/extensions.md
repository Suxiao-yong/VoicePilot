# VoicePilot 扩展配置指南（Extension / MCP Plugin / User Skill）

VoicePilot 的信任内核（Trust Kernel）把三类扩展统一进 `ExtensionCatalog`：
**内置 Skill（Builtin）**、**用户 Skill（User Skill 文件）** 和 **MCP Plugin（`mcp_servers` 表）**。
每条任务执行时取得不可变的 catalog snapshot；任何扩展调用都经过 Trust Kernel
的 Policy / Approval / Execution / Verify / Audit / Compensation，**Plugin 不得绕过 Trust Kernel**。

- 事实来源：SQLite `skills` / `mcp_servers` 表 + 用户 Skill 文件（`%APPDATA%\voicepilot\skills\*.md`）。
- 运行时缓存：`TrustKernel::extension_catalog`（`Arc<RwLock<ExtensionCatalog>>`），不单独持久化。
- 变更入口（toggle / import / register / remove）成功后会 `reload_extensions()`；失败不改变数据库或运行时 catalog。

---

## 1. 用户 Skill（User Skill）—— Agent Skills 开放标准

用户 Skill 遵循 **Agent Skills 开放标准**（agentskills.io，与 Claude Code / Codex / ChatGPT /
Gemini 一致）：目录式 `%APPDATA%\voicepilot\skills\{name}\SKILL.md`，
frontmatter 为**标准六字段**（`name` / `description` / `license` / `compatibility` /
`metadata` / `allowed-tools`），Markdown 正文为技能指令。

**最小示例（与 Claude Code / Codex 完全互通，均可直接放入使用）：**

```yaml
---
name: example-readonly
description: 只读示例扩展,调用 example-readonly server 的 read_item 工具读取条目。
---

# 示例只读扩展

正文说明……
```

**安全与执行（2026-08-24 统一后不再写入文件）：**

- **前端路由**：标准 `name` 即技能 id（约束 `^[a-z][a-z0-9._-]{0,63}$`）；`description`
  用于路由（自动派生关键词）+ LLM classify。
- **默认安全兜底**：文件不携带风险等级/审批/补偿等字段，系统默认 `E1 / D2 / local_only /
  max_steps 8 / 单步提交审批 / weak 验证 / stop 失败策略`。
- **可选执行绑定**（VoicePilot 扩展，标准客户端忽略 `metadata` 不报错）——把技能绑定到某个
  已注册的 `enabled + trusted` MCP server 工具：

```yaml
---
name: example-readonly
description: 只读示例扩展,调用 example-readonly server 的 read_item 工具。
metadata:
  voicepilot:
    execution:
      type: mcp_tool
      server_id: "example-readonly"
      tool_name: "read_item"
---

# 示例只读扩展
```

**关键规则：**

- **没有 `metadata.voicepilot.execution` 的 Skill 只作为不可执行的展示型技能**，不进入
  planner 候选，也不允许被 executor 直接调用。
- `execution.server_id` 指向的 MCP server 必须 `enabled + trusted` 且结构合法，否则该 Skill
  同样不进入候选。
- **旧版单文件 `skills\*.md` 格式已移除**（2026-08-24）。迁移：把文件放入
  `skills\<name>\SKILL.md`，frontmatter 改写为标准六字段；安全字段不再携带（走默认 +
  UI 配置）。
- 单文件上限 1 MiB；单文件解析失败仅跳过该技能，不影响其他技能加载。

## 2. MCP Plugin

MCP Plugin 就是 `mcp_servers` 表中的一行（不新建重复表）。UI 入口在 **Trust Center**，
命令：`register_mcp_server` / `remove_mcp_server` / `list_mcp_servers` / `toggle_mcp_server`。

### 字段

| 字段 | 说明 |
|---|---|
| `server_id` | 唯一标识，被 User Skill `execution.server_id` 引用 |
| `name` / `version` | 展示信息 |
| `transport` | 当前仅 `stdio` |
| `enabled` | 是否启用（toggle 切换） |
| `trusted` | 信任标记；非 trusted 的 server 不进入可执行候选 |
| `protocol_version` | 锁定值 `2025-11-25` |
| `command` / `args` | 启动子进程的命令与参数（JSON 字符串数组，边界解析一次） |
| `env` | 环境变量（JSON 字符串→字符串对象）；**密钥，绝不入审计** |
| `allowed_origins` / `allowed_paths` | JSON 字符串数组；文件路径需在 allowed_paths 白名单内 |

### 行为

- **删除被 User Skill execution target 引用的 server 会被拒绝**，并返回引用它的 Skill ID。
- **禁用（toggle off）会使依赖它的 User Skill 立即失去执行候选资格**（catalog 重建）。
- 删除“当前正在运行任务的 server”的保护需 per-task 快照追踪（schema 未持久化），当前
  未实现；已实现的保护是上面的引用检查。
- 每次 User-Skill MCP 调用前做 `tools/list` 预检并维护**会话 schema-hash 基线**：
  - 首次调用记录工具 schema 的 SHA-256；
  - 后续调用哈希不一致 → 拒绝并要求 **re-plan / re-approve**；
  - 显式配置变更（toggle 或重新注册 server）会重置该基线。
- 每次调用受 `McpCallLimits` 约束（默认 60s 超时、4 MiB 输出上限，均为常量，无 DB 字段）。

## 3. 执行与信任链

User-Skill MCP 调用经过的安全链：

```text
server allowlist(enabled+trusted)
→ tools/list 预检 + schema 基线
→ McpCallLimits(超时 / 输出上限)
→ result validation
→ taint / audit
```

> 说明：计划的 “manifest/risk → Gateway::decide” 步骤不适用于本路径 —— 既有 MCP 链从未
> 包含它们，且 MCP 调用没有可供 Cedar 决策的 effect manifest（已批准偏差，见 progress.md）。
> 已批准计划不会因用户切换插件而改变；运行中的任务使用任务创建时的 snapshot。

## 4. 信任内核仍负责

- **Verifier**：`model.onnx` / `tokens.txt` 完整性、SHA-256、`verify_move` 强校验等。
- **Compensation**：`compensations` 表 CRUD、`auto_reverse` 冲突策略；Stronghold 加密的
  reverse payload 仅服务于 compensation / privacy vault，与 keyring（SecretStore）互不替代。
- **Risk / Egress**：manifest 声明的 `risk_ceiling` / `data_class_ceiling` / `egress` 参与
  DAG 规划与展示。
- **Taint**：LLM / MCP 产出的值按 hash 传播 taint，写入 `LocalFile` 等 sink 前拦截。
- **Audit**：每次调用留下 `taint_propagated`（details 含 extension 元数据：
  `manifest_hash` / `source` / `snapshot_id`，MCP 另含 `server_id` / `tool_name`）。

## 5. 模型与 SecretStore（Wave 3）

- LLM API key 存在 Windows Credential Manager（`keyring`），**SQLite 不再存明文**；
  读取接口只返回 `llm_api_key_present`，写入接口接受 `llm_api_key` / `clear_llm_api_key`。
- 旧明文 `app_config.llm.api_key` 在启动时自动迁移到 keyring（成功后删除明文并写审计；
  失败则保留旧值并禁用 LLM）。
- 语音模型 canonical 目录 `%USERPROFILE%\.voicepilot\models`；旧
  `%LOCALAPPDATA%\voicepilot\models` 被兼容探测（不复制大文件）。
