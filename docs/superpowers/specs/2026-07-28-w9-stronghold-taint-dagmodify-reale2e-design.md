# W9: Stronghold 加密 + Taint Tracking + DAG Modify + 真实 E2E Design Spec

**日期:** 2026-07-28(Asia/Shanghai)
**对应规格:** V1.1.2 §7.2 强补偿 + §7.3 taint tracking + §2.7 DAG 骨架审批 + §8 延后项
**前置:** W8 已完成(commit `8ec814d`,W8 Plan 1-6 全部验收门禁关闭,465 非门控测试通过)
**范围:** 引入 tauri-plugin-stronghold 真实加密 `snapshot_encrypted` + 实现 taint tracking 污点传播 CRUD + 补齐 DAG Modify 审批分支 + 真实 Playwright/UIA 环境下 DAG 端到端验收;闭合 W8 spec §8 中 5 项延后到 W9 的工作。

---

## 1. 背景

W8 完成了 Skill 编排 + DAG 调度器,但 spec §8 列出 5 项延后到 W9 的工作:

1. **`snapshot_encrypted` 明文 PoC(W3a-W8 累计 4 个里程碑技术债)** — spec §7.2 #18 强制安全门禁,`compensations.snapshot_encrypted` 列已存在但永远为 `None`,reverse_payload 以明文 JSON 字符串落盘
2. **Taint Tracking 空壳表** — `taints` 表 schema 已建但零 CRUD 代码,gateway.rs:79 仅一处硬编码 `web_page provenance` 规则
3. **DAG Modify UI 占位** — W8 Plan 5 `DagApprovalDecision::Modify` 变体已定义但前端 disabled + 标注 "W9+",`approve_dag_skeleton` 签名无 modified payload 回传通道
4. **真实 E2E 验证缺失** — W8 Plan 6 的 8 个场景全用 mock LLM + mock MCP,真实 Playwright/UIA 环境下 DAG 串联未验证
5. **DAG Modify 后审批语义未定义** — 用户修改 DAG 骨架后是否重新走完整 skeleton 审批未明确

W9 闭合以上 5 项,引入 StrongholdVault(密钥管理 + 加密)+ TaintRepo(CRUD + 查表驱动)+ DagApprovalOutcome(Modify payload 回传)+ 真实 E2E 测试基础设施(`#[ignore]` 模式),复用 W7-W8 的 TrustKernel + DagExecutor + SlotTemplateEngine + TauriApprover 基础设施。

**用户决策(2026-07-28)W9 范围确认:**

| # | 维度 | 决策 |
|---|---|---|
| 1 | Stronghold 加密范围 | 仅加密 `reverse_payload`(compensations 表);fs_snapshot 采集延后 W10+ |
| 2 | 密钥管理策略 | 用户密码派生((Argon2id + Stronghold SaltClientHash),无服务端密钥 |
| 3 | 密码丢失语义 | 丢失密码 = reverse_payload 永久不可解;启动时密码错误 = 降级运行(不加载补偿记录) |
| 4 | Taint Tracking 范围 | 完整 CRUD + 升级 gateway.rs 硬编码为查表驱动;Cedar 引擎 taint 查询延后 W10+ |
| 5 | Taint 传播范围 | 仅 Skill 输入 → 输出传播(用户输入 → executor 输出);文件系统 sink 延后 W10+ |
| 6 | DAG Modify 范围 | 允许增删节点 + 改 input_template + 改 risk_ceiling;改 loop_specs 延后 W10+ |
| 7 | Modify 后审批流程 | 重新走完整 skeleton 审批(spec §2.7 语义,Modify 后视为"新"DAG) |
| 8 | 真实 E2E 测试触发 | `#[ignore]` 手动运行(W7 既有模式,无 CI) |
| 9 | 真实 E2E 覆盖场景 | note.capture + files.organize(UIA)+ form.prepare + form.submit(Playwright) |
| 10 | Slot 流水欠债 | W9 Plan 6 闭合 `user_slots` 在 DAG executor 中传 `&[]` 的欠债 |

---

## 2. 范围

W9 共 7 项工作(7 个 Plan):

| # | 项 | 优先级 | Plan |
|---|---|---|---|
| 1 | Stronghold 加密基础 + 密钥管理(Argon2id + Stronghold) | P0 | Plan 1 |
| 2 | snapshot_encrypted 真实加密 + 明文 PoC 移除 | P0 | Plan 2 |
| 3 | Taint Tracking 污点传播(CRUD + 查表驱动 gateway) | P0 | Plan 3 |
| 4 | DAG Modify 分支实现(后端 + UI + 重新审批) | P0 | Plan 4 |
| 5 | 真实 Playwright MCP DAG E2E | P1 | Plan 5 |
| 6 | 真实 UIA GUI DAG E2E + Slot 流水闭合 | P1 | Plan 6 |
| 7 | 集成测试 + 6 套 feature cargo check + clippy + npm build + PROGRESS.md 收尾 | P0 | Plan 7 |

### 2.1 Stronghold 加密基础 + 密钥管理(Plan 1,P0)

**新增文件:** `crates/trust-kernel/src/crypto/stronghold.rs`

**职责:** 封装 `tauri-plugin-stronghold`,提供 `StrongholdVault` 抽象,管理 reverse_payload 加密 / 解密 / 密钥派生。

**数据结构:**

```rust
pub struct StrongholdVault {
    inner: Mutex<Option<Stronghold>>,  // None = 未解锁 / 降级模式
    salt: Salt,                        // 持久化在 KV(config table)
    unlocked: AtomicBool,
}

pub enum StrongholdError {
    NotUnlocked,                       // 降级模式或未调用 unlock
    WrongPassword,                     // Argon2id 派生密钥不匹配
    EncryptionFailed(String),          // Stronghold 内部错误
    DecryptionFailed(String),
    VaultCorrupted(String),            // vault 文件损坏
}

pub struct EncryptedPayload {
    pub ciphertext: Vec<u8>,           // Stronghold 加密后的密文
    pub nonce: Vec<u8>,                // XSalsa20Poly1305 nonce
    pub salt_ref: String,              // 指向 KV 中存储的 salt
}
```

**API:**

```rust
impl StrongholdVault {
    /// 创建新 vault(首次启动时调用)。生成随机 salt + 持久化到 config table。
    pub fn create(password: &str, db: &Connection) -> Result<Self>;

    /// 用密码解锁已有 vault。密码错误返回 WrongPassword。
    pub fn unlock(&self, password: &str) -> Result<()>;

    /// 锁定 vault(清空内存中的 key material)。
    pub fn lock(&self);

    /// 加密 plaintext → EncryptedPayload。未解锁时返回 NotUnlocked。
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<EncryptedPayload>;

    /// 解密 EncryptedPayload → plaintext。
    pub fn decrypt(&self, payload: &EncryptedPayload) -> Result<Vec<u8>>;

    /// 是否已解锁。
    pub fn is_unlocked(&self) -> bool;

    /// 降级模式构造(无密码启动,不加载补偿记录)。
    pub fn degraded() -> Self;
}
```

**密钥派生策略(用户决策 #2):**

```
user_password (用户输入)
    ↓ Argon2id(m=64MB, t=3, p=4, salt=persisted_salt)
derived_key (32 bytes)
    ↓ Stronghold SaltClientHash
stronghold_master_key
    ↓ Stronghold internal
vault encryption key (XSalsa20Poly1305)
```

**配置存储:**

| Key | 位置 | 值 |
|---|---|---|
| `stronghold.salt` | `config` 表(KV) | base64(Argon2id salt, 16 bytes) |
| `stronghold.vault_path` | `config` 表 | `${data_dir}/stronghold.bin`(默认) |
| `stronghold.enabled` | `config` 表 | `"true"` / `"false"`(默认 true,可禁用回退明文 PoC 用于测试) |

**降级模式语义(用户决策 #3):**

- 启动时调用 `StrongholdVault::unlock(password)`,密码错误 → `vault = StrongholdVault::degraded()`
- 降级模式下:`create_post_commit_compensation` 跳过加密,`snapshot_encrypted = None` + `snapshot_vault_ref = "degraded"`(可识别降级模式)
- 降级模式下:`reverse_compensation` 拒绝执行(无法解密历史 payload),返回 `StrongholdError::NotUnlocked`
- 降级模式审计事件:`stronghold_degraded_mode_entered` with `{reason: "wrong_password" / "vault_corrupted"}`

**Feature flag:**

```toml
# trust-kernel/Cargo.toml
[features]
default = ["llm"]
stronghold = ["dep:tauri-plugin-stronghold", "dep:argon2"]

[dependencies]
tauri-plugin-stronghold = { workspace = true, optional = true }
argon2 = { version = "0.5", optional = true }
```

```toml
# workspace Cargo.toml
[workspace.dependencies]
tauri-plugin-stronghold = "2"
argon2 = "0.5"
```

**与 privacy_mode 联动(用户决策 #1 + 核实报告 1.5):**

- `privacy_mode = true` 时,强制要求 Stronghold 已解锁,否则启动失败(防止高隐私模式下 reverse_payload 明文落盘)
- `privacy_mode = false` 时,Stronghold 降级模式允许启动,但审计记录降级原因

### 2.2 snapshot_encrypted 真实加密 + 明文 PoC 移除(Plan 2,P0)

**修改文件:** `crates/trust-kernel/src/skills/common.rs`

**当前明文 PoC(核实报告 1.1):**

```rust
// common.rs:96-129(当前)
pub fn create_post_commit_compensation(...) -> Result<CompensationRecord> {
    // ...
    let reverse_payload_json = serde_json::to_string(&reverse_payload)?;
    // 明文写入 reverse_payload 列(无加密)
    let record = CompensationRecord {
        snapshot_encrypted: None,        // ← 永远 None(W9 修复)
        snapshot_vault_ref: None,        // ← 永远 None(W9 修复)
        reverse_payload: reverse_payload_json,
        // ...
    };
}
```

**W9 修复后:**

```rust
pub fn create_post_commit_compensation(
    kernel: &TrustKernel,
    // ... 既有参数
) -> Result<CompensationRecord> {
    let reverse_payload_json = serde_json::to_string(&reverse_payload)?;

    // W9: Stronghold 加密 reverse_payload
    let (snapshot_encrypted, snapshot_vault_ref) = if kernel.stronghold_enabled() {
        let vault = kernel.stronghold_vault();
        if vault.is_unlocked() {
            let payload = vault.encrypt(reverse_payload_json.as_bytes())?;
            let payload_bytes = bincode::serialize(&payload)?;
            let vault_ref = uuid::Uuid::new_v4().to_string();
            (Some(payload_bytes), Some(vault_ref))
        } else {
            // 降级模式:记录但不加密
            (None, Some("degraded".to_string()))
        }
    } else {
        // stronghold feature 未启用:保持明文 PoC 行为(测试用)
        (None, None)
    };

    // reverse_payload 列保留明文(向后兼容,但 W9 验收门禁要求 stronghold feature 启用时此列为空)
    // 若 stronghold_enabled && unlocked,reverse_payload = ""(空字符串),snapshot_encrypted 含密文
    let stored_reverse_payload = if snapshot_encrypted.is_some() {
        String::new()  // 密文已存 snapshot_encrypted,明文不落盘
    } else {
        reverse_payload_json  // 降级或 feature 未启用,明文落盘
    };

    let record = CompensationRecord {
        snapshot_encrypted,
        snapshot_vault_ref,
        reverse_payload: stored_reverse_payload,
        // ...
    };
    // INSERT INTO compensations ...
}
```

**reverse_compensation 修改:**

```rust
pub fn reverse_compensation(kernel: &TrustKernel, compensation_id: &str) -> Result<()> {
    let record = CompensationRepo::get(compensation_id)?;
    let reverse_payload_json = if let Some(encrypted) = &record.snapshot_encrypted {
        // W9: 从密文解密
        let vault = kernel.stronghold_vault();
        if !vault.is_unlocked() {
            return Err(StrongholdError::NotUnlocked.into());
        }
        let payload: EncryptedPayload = bincode::deserialize(encrypted)?;
        let plaintext = vault.decrypt(&payload)?;
        String::from_utf8(plaintext)?
    } else {
        // 降级模式或 feature 未启用:用明文 reverse_payload
        record.reverse_payload.clone()
    };
    // ... 执行 reverse
}
```

**审计事件:**

| event_type | details | 触发时机 |
|---|---|---|
| `stronghold_snapshot_encrypted` | `{compensation_id, vault_ref, plaintext_len}` | create_post_commit_compensation 加密成功 |
| `stronghold_snapshot_decrypt_failed` | `{compensation_id, error}` | reverse_compensation 解密失败 |
| `stronghold_degraded_mode_entered` | `{reason}` | 启动时密码错误或 vault 损坏 |

**明文残留检测(W9 验收门禁):**

```powershell
# W9 验收脚本:grep 不到明文 reverse_payload(当 stronghold feature 启用 + vault 解锁时)
SELECT COUNT(*) FROM compensations
WHERE snapshot_encrypted IS NULL
  AND snapshot_vault_ref IS NULL
  AND reverse_payload != ''
-- stronghold feature 启用时,此 COUNT 必须 = 0
```

### 2.3 Taint Tracking 污点传播(Plan 3,P0)

**新增文件:** `crates/trust-kernel/src/policy/taint_repo.rs`

**职责:** 实现 `taints` 表的 CRUD + 查表驱动 gateway 规则 + Skill 输入 → 输出 taint 传播。

**数据结构(复用现有 `taints` 表 schema,核实报告 1.6):**

```sql
-- migrations/001_init.sql:98-105(已存在,无需 migration)
CREATE TABLE taints (
    taint_id TEXT PRIMARY KEY,
    value_hash TEXT NOT NULL,           -- SHA256(value) 用于去重
    provenance TEXT NOT NULL,           -- "user_input" / "web_page" / "llm_output" / "mcp_tool:playwright"
    taints_json TEXT NOT NULL,          -- JSON array of taint labels
    collected_at TEXT NOT NULL,
    source_ref TEXT                     -- task_id / step_id / node_id
);
```

```rust
// taint_repo.rs(新增)
pub struct TaintRepo;

impl TaintRepo {
    pub fn new() -> Self;

    /// 插入一条 taint 记录。若 value_hash 已存在,合并 taints_json(去重)。
    pub fn upsert(&self, conn: &Connection, taint: &TaintRecord) -> Result<()>;

    /// 按 value_hash 查询 taints。
    pub fn find_by_value(&self, conn: &Connection, value: &str) -> Result<Option<TaintRecord>>;

    /// 按 provenance 查询所有 taints。
    pub fn list_by_provenance(&self, conn: &Connection, provenance: &str) -> Result<Vec<TaintRecord>>;

    /// 按 source_ref 查询(如某 task_id 关联的所有 taints)。
    pub fn list_by_source(&self, conn: &Connection, source_ref: &str) -> Result<Vec<TaintRecord>>;

    /// 删除某 source_ref 的所有 taints(任务清理时用)。
    pub fn delete_by_source(&self, conn: &Connection, source_ref: &str) -> Result<u64>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaintRecord {
    pub taint_id: String,
    pub value_hash: String,
    pub provenance: String,
    pub taints: Vec<String>,            // ["user_input", "sensitive"]
    pub collected_at: String,
    pub source_ref: Option<String>,
}
```

**Taint 传播规则(用户决策 #5):**

| 传播点 | 输入 provenance | 输出 provenance | 规则 |
|---|---|---|---|
| Skill executor 输入 | `user_input` | (输出继承输入 taints) | executor 输出 value_hash = SHA256(output_value);若输入有 taint,输出 taints = 输入 taints + `executor_output:<skill_id>` |
| LLM 拆解 | (用户语音文本) | `llm_output` | LLM 生成的 DagPlan 中所有 literal 值都标记为 `llm_output` taint |
| MCP tool 调用 | (executor 输入) | `mcp_tool:<server_id>` | MCP 返回值标记为 `mcp_tool:<server_id>` taint |
| 文件系统写入 | (executor 输入) | (W10+ sink,暂不传播) | 文件路径不标 taint(避免污染) |

**Skill 输入 → 输出传播实现:**

```rust
// skills/dispatcher.rs(修改)
pub fn dispatch_skill_executor(
    kernel: &TrustKernel,
    skill_id: &str,
    input: &serde_json::Value,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    // W9: 记录输入 taint
    let input_str = serde_json::to_string(input)?;
    let input_hash = sha256(&input_str);
    let input_taints = TaintRepo::new()
        .find_by_value(&kernel.conn(), &input_hash)?
        .map(|r| r.taints)
        .unwrap_or_default();

    let outcome = /* 既有 executor 调用 */;

    // W9: 记录输出 taint(继承输入 + 加 executor_output 标签)
    if let Some(output_value) = outcome.output_value() {
        let output_str = serde_json::to_string(&output_value)?;
        let output_hash = sha256(&output_str);
        let mut output_taints = input_taints.clone();
        output_taints.push(format!("executor_output:{}", skill_id));
        output_taints.dedup();

        TaintRepo::new().upsert(&kernel.conn(), &TaintRecord {
            taint_id: uuid::Uuid::new_v4().to_string(),
            value_hash: output_hash,
            provenance: format!("executor_output:{}", skill_id),
            taints: output_taints,
            collected_at: now_iso8601(),
            source_ref: Some(format!("{}:{}", task_id, step_id)),
        })?;
    }

    Ok(outcome)
}
```

**Gateway 查表驱动(替换 gateway.rs:79 硬编码):**

```rust
// gateway.rs(修改)
// 当前(硬编码):
// if resource.provenance == "web_page" && sink == "ToolArgument" { return Err(...); }

// W9: 查表驱动
pub fn check_taint_policy(
    conn: &Connection,
    resource: &Resource,
    sink: &Sink,
) -> Result<()> {
    let taints = TaintRepo::new()
        .find_by_value(conn, &resource.value_hash)?
        .map(|r| r.taints)
        .unwrap_or_default();

    // 规则:web_page taint 不能成为 ToolArgument(防止 LLM 投毒)
    if taints.iter().any(|t| t == "web_page") && sink == &Sink::ToolArgument {
        return Err(GatewayError::TaintPropagationBlocked {
            taints: taints.clone(),
            sink: sink.clone(),
        });
    }

    // 规则:llm_output taint 不能直接写入文件系统(防止 LLM 注入恶意路径)
    if taints.iter().any(|t| t == "llm_output") && sink == &Sink::FilesystemWrite {
        return Err(GatewayError::TaintPropagationBlocked {
            taints: taints.clone(),
            sink: sink.clone(),
        });
    }

    Ok(())
}
```

**审计事件:**

| event_type | details | 触发时机 |
|---|---|---|
| `taint_propagated` | `{source_ref, input_hash, output_hash, taints}` | dispatcher 传播 taint |
| `taint_blocked` | `{taints, sink, resource_hash}` | gateway 拦截 taint 传播 |

### 2.4 DAG Modify 分支实现(Plan 4,P0)

**修改文件:**
- `crates/trust-kernel/src/approval/approver.rs`(扩展 trait 签名)
- `crates/trust-kernel/src/skills/dag_executor.rs`(处理 Modify 分支)
- `crates/ui/src/dag_commands.rs`(扩展命令签名)
- `crates/ui/src/approver.rs`(TauriApprover 携带 modified_plan)
- `crates/ui/web/src/components/DagApprovalDialog.tsx`(激活 Modify 按钮 + 编辑器)
- `crates/ui/web/src/api.ts`(扩展 IPC 调用)

**当前占位(核实报告 2.1-2.5):**

```rust
// approver.rs:41(当前)
fn approve_dag_skeleton(&self, plan: &DagPlan) -> Result<ApprovalDecision>;

// dag_commands.rs:23-30(当前)
pub enum DagApprovalDecision {
    Allow,
    Deny,
    /// W9+ 实现(spec §8 延后项):用户调整 input_template
    Modify,
}

// submit_dag_skeleton_approval(当前)只发 decision,无 modified payload
```

**W9 修复后:**

```rust
// approver.rs(W9)
pub enum DagApprovalOutcome {
    Allow,
    Deny,
    Modify {
        modified_plan: Box<DagPlan>,
    },
}

pub trait Approver: Send + Sync {
    // 既有方法...
    fn approve_dag_skeleton(&self, plan: &DagPlan) -> Result<DagApprovalOutcome>;
}

// dag_executor.rs(W9 处理 Modify)
fn run(&self, plan: &DagPlan) -> Result<DagResult> {
    let outcome = self.approver.approve_dag_skeleton(plan)?;
    let effective_plan = match outcome {
        DagApprovalOutcome::Allow => plan.clone(),
        DagApprovalOutcome::Deny => {
            // 既有 Cancelled 路径
            return Ok(DagResult { status: DagStatus::Cancelled, ... });
        }
        DagApprovalOutcome::Modify { modified_plan } => {
            // W9: 审计 Modify + 重新走 skeleton 审批(用户决策 #7)
            self.audit_append_external(
                "dag_skeleton_modified",
                json!({plan_id: plan.plan_id, modified_node_count: modified_plan.nodes.len()}),
            )?;
            // 重新校验 modified_plan(SlotTemplateEngine::validate_dag)
            SlotTemplateEngine::validate_dag(&modified_plan)?;
            // 递归调用 run(modified_plan)?  — 但需防无限递归
            // 实际:Modify 只允许一次,第二次直接 Allow/Deny
            return self.run_modified(&modified_plan);
        }
    };
    // 既有执行逻辑...
}

fn run_modified(&self, modified_plan: &DagPlan) -> Result<DagResult> {
    // 第二次审批只允许 Allow/Deny(不再允许 Modify)
    let outcome = self.approver.approve_dag_skeleton(modified_plan)?;
    match outcome {
        DagApprovalOutcome::Allow => { /* 执行 modified_plan */ }
        DagApprovalOutcome::Deny => { /* Cancelled */ }
        DagApprovalOutcome::Modify { .. } => {
            // 第二次 Modify 拒绝(防止无限递归)
            return Err(KernelError::DagModifyLimitExceeded);
        }
    }
}
```

**UI 修改(W9 Plan 4):**

```tsx
// DagApprovalDialog.tsx(W9)
// 当前(占位):<button disabled title="W9+ 实现">调整(Modify)· W9+</button>
// W9:激活 + 弹出编辑器

function DagApprovalDialog({ request, onAllow, onDeny, onModify }: Props) {
    const [editingMode, setEditingMode] = useState(false);
    const [editedNodes, setEditedNodes] = useState(request.nodes);

    const handleModifySubmit = () => {
        const modifiedPlan = { ...request, nodes: editedNodes };
        onModify(modifiedPlan);
    };

    return (
        <Modal>
            {!editingMode ? (
                <>
                    {/* 既有节点卡片展示 */}
                    <button onClick={() => setEditingMode(true)}>调整(Modify)</button>
                    <button onClick={onAllow}>Allow</button>
                    <button onClick={onDeny}>Deny</button>
                </>
            ) : (
                <>
                    {/* W9:可编辑节点列表(增删 + input_template textarea + risk_ceiling select) */}
                    {editedNodes.map(node => (
                        <NodeEditor
                            key={node.node_id}
                            node={node}
                            onChange={updated => setEditedNodes(prev => prev.map(n => n.node_id === updated.node_id ? updated : n))}
                            onDelete={() => setEditedNodes(prev => prev.filter(n => n.node_id !== node.node_id))}
                        />
                    ))}
                    <button onClick={() => setEditedNodes(prev => [...prev, newEmptyNode()])}>+ 添加节点</button>
                    <button onClick={handleModifySubmit}>提交修改</button>
                    <button onClick={() => setEditingMode(false)}>取消</button>
                </>
            )}
        </Modal>
    );
}
```

**TauriApprover 扩展:**

```rust
// crates/ui/src/approver.rs(W9)
impl Approver for TauriApprover {
    fn approve_dag_skeleton(&self, plan: &DagPlan) -> Result<DagApprovalOutcome> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let request_id = self.registry.create_request(tx)?;

        // emit 事件给 webview,携带完整 plan(供前端编辑)
        self.app.emit("dag-approval-request", DagApprovalRequestPayload {
            approval_request_id: request_id.clone(),
            plan: plan.clone(),
        })?;

        // 5min timeout,默认 Deny(既有模式)
        match self.registry.wait_for_decision(&request_id, rx, Duration::from_secs(300)) {
            Ok(decision_payload) => {
                // W9:决策携带 modified_plan
                match decision_payload.decision {
                    DagApprovalDecision::Allow => Ok(DagApprovalOutcome::Allow),
                    DagApprovalDecision::Deny => Ok(DagApprovalOutcome::Deny),
                    DagApprovalDecision::Modify => {
                        let modified = decision_payload.modified_plan
                            .ok_or_else(|| KernelError::InvalidApprovalPayload("Modify without modified_plan"))?;
                        Ok(DagApprovalOutcome::Modify { modified_plan: Box::new(modified) })
                    }
                }
            }
            Err(_) => Ok(DagApprovalOutcome::Deny),  // timeout → Deny
        }
    }
}
```

**审计事件:**

| event_type | details | 触发时机 |
|---|---|---|
| `dag_skeleton_modified` | `{plan_id, modified_node_count, added_count, removed_count}` | 用户提交 Modify |
| `dag_modify_limit_exceeded` | `{plan_id}` | 第二次 Modify 被拒绝 |

### 2.5 真实 Playwright MCP DAG E2E(Plan 5,P1)

**新增文件:** `crates/trust-kernel/tests/w9_plan5_playwright_dag_e2e.rs`

**测试场景:**

| 场景 | DAG | 验证点 |
|---|---|---|
| 真实 form DAG | `[form.prepare, form.submit]` | form.prepare 真实打开网页 + form.submit 真实点击 submit + E3 PerStep 审批 + Stronghold 加密补偿记录 |
| 真实 research DAG | `[research.save_markdown]` | 真实 Playwright 抓取网页 + 保存 markdown + taint 传播(web_page provenance) |

**测试模式(复用 W7 Plan 5,核实报告 3.1-3.2):**

```rust
// w9_plan5_playwright_dag_e2e.rs
#![cfg(feature = "voice,tauri,llm")]

#[tokio::test]
#[ignore = "Requires real npx + @playwright/mcp + network. Run: cargo test --features voice,tauri,llm --test w9_plan5_playwright_dag_e2e -- --ignored"]
async fn real_form_prepare_submit_dag_succeeds() {
    // 1. 探测 npx + @playwright/mcp 可用性,不可用则 short-circuit passing
    if !npx_playwright_available().await {
        eprintln!("Skipping: npx @playwright/mcp not available");
        return;
    }

    // 2. 启动真实 MCP playwright server(kernel 自动 seed,核实报告 3.3)
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    kernel.set_stronghold_vault(StrongholdVault::create("test_password", &kernel.conn()).unwrap());

    // 3. 构造 DAG:form.prepare → form.submit
    let dag_plan = DagPlan {
        plan_id: format!("w9-plan5-{}", uuid::Uuid::new_v4()),
        user_goal: "测试真实 Playwright DAG".into(),
        nodes: vec![
            DagNode {
                node_id: "n1".into(),
                skill_id: "form.prepare".into(),
                input_template: literal_text_template(r#"{"url": "https://httpbin.org/forms/post"}"#),
                risk_ceiling: ELevel::E2,
            },
            DagNode {
                node_id: "n2".into(),
                skill_id: "form.submit".into(),
                input_template: SlotTemplate {
                    kind: SlotKind::Text,
                    template: TemplateExpr::Concat(vec![
                        TemplateExpr::Literal(r#"{"url": ""#.into()),
                        TemplateExpr::Var(VarRef { scope: VarScope::Prev, path: "output.url".into() }),
                        TemplateExpr::Literal(r#""}"#.into()),
                    ]),
                },
                risk_ceiling: ELevel::E3,
            },
        ],
        edges: vec![DagEdge { from: "n1".into(), to: "n2".into(), port_binding: None }],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    };

    // 4. DagExecutor::run(AutoApprover)
    let executor = DagExecutor::new(kernel.clone(), Arc::new(AutoApprover), Arc::new(DagRepo::new()));
    let result = executor.run(&dag_plan).unwrap();

    // 5. 验证:DagStatus::Succeeded + 2 节点 Succeeded + Stronghold 加密补偿 + taint 传播
    assert!(matches!(result.status, DagStatus::Succeeded));
    assert!(result.node_results.get("n1").unwrap().is_succeeded());
    assert!(result.node_results.get("n2").unwrap().is_succeeded());

    // Stronghold:补偿记录的 snapshot_encrypted 非空
    let conn = kernel.conn();
    let encrypted_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM compensations WHERE snapshot_encrypted IS NOT NULL",
        [], |row| row.get(0)
    ).unwrap();
    assert!(encrypted_count > 0, "Stronghold encryption must produce non-null snapshot_encrypted");

    // Taint:form.submit 输出有 mcp_tool:playwright taint
    let taints = TaintRepo::new().list_by_provenance(&conn, "mcp_tool:playwright").unwrap();
    assert!(!taints.is_empty(), "Playwright MCP output must be taint-tracked");
}
```

### 2.6 真实 UIA GUI DAG E2E + Slot 流水闭合(Plan 6,P1)

**新增文件:** `crates/trust-kernel/tests/w9_plan6_uia_dag_e2e.rs`

**Slot 流水欠债闭合(核实报告 4.2-4.3):**

```rust
// dag_executor.rs(W9 修复)
// 当前:run_simple_node 传 user_slots: &[](行 354)
// W9:DagExecutor::run 签名扩展,接收 user_slots

pub fn run(&self, plan: &DagPlan, user_slots: &[ExtractedSlot]) -> Result<DagResult> {
    // ...
    self.run_simple_node(&node, &node_outputs, user_slots, prev_node_id)?;
    // ...
}

// IterableSource::UserSlot(W9 实现,当前返回 Err)
// 注意:IterableSource::UserSlot 是 struct variant { slot_kind: String },
// 不是 tuple variant(SlotKind)。错误类型统一为 TemplateError::UserSlotNotFound,
// 不要用 KernelError::Skill(format!(...)) 替代。
IterableSource::UserSlot { slot_kind } => {
    let slot = user_slots.iter().find(|s| s.kind.to_string() == slot_kind)
        .ok_or_else(|| TemplateError::UserSlotNotFound(slot_kind.clone()))?;
    // 解析 slot.raw 为 iterable(JSON 数组或 CSV)
}
```

**测试场景:**

| 场景 | DAG | 验证点 |
|---|---|---|
| 真实 note.capture DAG | `[note.capture]` | 真实打开记事本 + UIA 写 TODO + 保存桌面 + Stronghold 加密补偿 + taint 传播(user_input provenance) |
| 真实 note + files DAG | `[note.capture, files.organize]` | note.capture 输出路径 → files.organize input_template `${prev.output.save_path}` Slot 流水 + 真实文件移动 |

**测试模式(复用 W7 Plan 4,核实报告 3.1-3.2):**

```rust
// w9_plan6_uia_dag_e2e.rs
// 注意:feature gate 必须用多 feature 分别声明语法(`feature = "a", feature = "b"`),
// 不能用单字符串 `feature = "a,b"`(Rust cfg 不会解析为多 feature)。
#![cfg(all(windows, feature = "voice", feature = "tauri", feature = "llm", feature = "uia", feature = "stronghold"))]

#[tokio::test(flavor = "current_thread")]  // thread-local UiaAdapter 跨 await 点会丢失,必须单线程 runtime
#[ignore = "Requires real Windows GUI session + notepad.exe. Run: cargo test --features voice,tauri,llm,uia,stronghold --test w9_plan6_uia_dag_e2e -- --ignored"]
async fn real_note_capture_files_organize_dag_succeeds() {
    // 1. 探测 Windows GUI 会话(不可用则 short-circuit)
    if !windows_gui_available() {
        eprintln!("Skipping: no Windows GUI session");
        return;
    }

    // 2. 启动 kernel + Stronghold
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    kernel.set_stronghold_vault(StrongholdVault::create("test_password", &kernel.conn()).unwrap());

    // 3. 构造 DAG:note.capture → files.organize(Slot 流水)
    let dag_plan = DagPlan {
        plan_id: format!("w9-plan6-{}", uuid::Uuid::new_v4()),
        user_goal: "打开记事本写 TODO 然后保存到桌面".into(),
        nodes: vec![
            DagNode {
                node_id: "n1".into(),
                skill_id: "note.capture".into(),
                input_template: literal_text_template(r#"{"content": "W9 测试 TODO", "save_path": "C:/Users/Public/Desktop/w9_test.txt"}"#),
                risk_ceiling: ELevel::E1,
            },
            DagNode {
                node_id: "n2".into(),
                skill_id: "files.organize".into(),
                input_template: SlotTemplate {
                    kind: SlotKind::Files,
                    template: TemplateExpr::Var(VarRef {
                        scope: VarScope::Prev,
                        path: "output.save_path".into(),
                    }),
                },
                risk_ceiling: ELevel::E2,
            },
        ],
        edges: vec![DagEdge { from: "n1".into(), to: "n2".into(), port_binding: None }],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    };

    // 4. DagExecutor::run(AutoApprover, user_slots=[])
    let executor = DagExecutor::new(kernel.clone(), Arc::new(AutoApprover), Arc::new(DagRepo::new()));
    let result = executor.run(&dag_plan, &[]).unwrap();

    // 5. 验证:Slot 流水(n2 input = n1 output.save_path)+ Stronghold 加密 + taint 传播
    assert!(matches!(result.status, DagStatus::Succeeded));
    // ... 既有断言
}
```

### 2.7 集成验收(Plan 7,P0)

**新增文件:**
- `crates/trust-kernel/tests/w9_stronghold_unit.rs`(Stronghold 加密单元测试)
- `crates/trust-kernel/tests/w9_taint_tracking_unit.rs`(Taint CRUD 单元测试)
- `crates/trust-kernel/tests/w9_dag_modify_smoke.rs`(DAG Modify 闭环测试)
- `crates/trust-kernel/tests/w9_default_boundary_smoke.rs`(default 组合边界测试,闭合 ≥ 286 阈值)

**验收门禁(W9):**

| 门禁类别 | 指标 | 阈值 |
|---|---|---|
| 编译 | 7 套 feature 组合 cargo check | 全 PASS(default / +llm / +tauri / +voice,tauri / +voice,tauri,llm / +voice,tauri,llm,uia / +voice,tauri,llm,uia,stronghold) |
| clippy | 2 套代表性 feature 组合 `-D warnings` | 0 警告 |
| npm build | `npm.cmd run build` | PASS |
| 非门控测试数 | `cargo test --workspace --no-default-features -- --list` | ≥ 286 |
| Stronghold 加密 | `SELECT COUNT(*) FROM compensations WHERE snapshot_encrypted IS NULL AND reverse_payload != ''` | = 0(stronghold feature 启用时) |
| Taint Tracking | `taints` 表有 CRUD 代码 + gateway 查表驱动 | grep `INSERT INTO taints` 非空 |
| DAG Modify | Modify → 重新审批 → 执行闭环 | w9_dag_modify_smoke 通过 |
| 真实 E2E | `#[ignore]` 测试手动运行通过 | 2 个场景(form DAG + note DAG) |
| 哈希链 | audit_logs 哈希链不断 | w9 验收脚本验证 |

---

## 3. 依赖图与执行顺序

```
Plan 1 (Stronghold 基础 + 密钥管理)
    ↓
Plan 2 (snapshot_encrypted 真实加密)          Plan 4 (DAG Modify)
    ↓                                              ↓
Plan 3 (Taint Tracking)                            │
    ↓                                              ↓
    └─────────────────────────────────────────────┴──
                          ↓
              Plan 5 (真实 Playwright E2E)  ← 依赖 Plan 2(stronghold)+ Plan 4(DAG Modify 完整)
              Plan 6 (真实 UIA E2E + Slot 流水闭合)  ← 同上
                          ↓
              Plan 7 (集成验收)
```

**并行执行策略:**

| 阶段 | Plan | 依赖 | 可并行 |
|---|---|---|---|
| 阶段 1 | Plan 1(Stronghold 基础) | 无 | 与 Plan 4 并行 |
| 阶段 1 | Plan 4(DAG Modify) | W8 Plan 5 ✅ | 与 Plan 1 并行 |
| 阶段 2 | Plan 2(snapshot_encrypted 加密) | Plan 1 ✅ | — |
| 阶段 2 | Plan 3(Taint Tracking) | Plan 2 ✅(复用审计基础设施) | — |
| 阶段 3 | Plan 5(Playwright E2E) | Plan 2 ✅ + Plan 4 ✅ | 与 Plan 6 并行 |
| 阶段 3 | Plan 6(UIA E2E) | Plan 2 ✅ + Plan 4 ✅ | 与 Plan 5 并行 |
| 阶段 4 | Plan 7(验收) | Plan 1-6 ✅ | — |

---

## 4. 风险登记

| 风险 | 可能性 | 影响 | 缓解 |
|---|---|---|---|
| Stronghold 加密时序改造破坏 W3a-W8 既有 compensation 调用 | 中 | 高 | Plan 2 用 feature flag 渐进切换;stronghold feature 未启用时保持明文 PoC 行为(测试兼容) |
| Argon2id 密钥派生性能差(64MB 内存)导致启动慢 | 中 | 中 | Plan 1 提供 `stronghold.kdf_memory` config 项,默认 64MB 可降到 16MB;benchmark 测试 |
| 密码丢失 = 补偿记录永久不可解 | 高 | 高 | 用户决策 #3 接受此语义;UI 明确警告;审计记录降级模式进入 |
| Taint Tracking 与 Slot 流水数据流图概念重叠导致双份实现 | 中 | 中 | Plan 3 brainstorming 阶段明确:Slot 流水是"节点间数据传递",Taint 是"值级污点追踪",两者正交 |
| DAG Modify 前端编辑复杂度高 | 中 | 中 | Plan 4 先做最小可用(仅改 input_template),增删节点 + risk_ceiling 留 Plan 4 后期 |
| 真实 Playwright MCP 环境不稳定 | 高 | 中 | `#[ignore]` 标记,手动运行,失败不阻塞 CI |
| 真实 UIA GUI 测试依赖 Windows GUI 会话 | 高 | 中 | 同上,文档化运行前置条件 |
| Slot 流水闭合(user_slots 传 `&[]`)可能破坏 W8 既有测试 | 中 | 中 | Plan 6 用 `&[]` 默认值保持向后兼容;新参数为 `Option<&[ExtractedSlot]>` |
| Plan 4 DAG Modify 与 Plan 1-3 并行可能导致 audit_log 字段冲突 | 低 | 低 | Plan 4 须在 brainstorming 阶段确认 `dag_skeleton_modified` 事件与 Stronghold 审计事件无字段冲突 |
| `tauri-plugin-stronghold` 在 Windows 上的二进制兼容性 | 中 | 高 | Plan 1 首先验证 `cargo check --features stronghold` 在 Windows 上通过;若不通过,降级为 `ring` + `aes-gcm` 自实现 |

---

## 5. Fitness Functions(验收标准)

| 属性 | 指标 | 阈值 | 测量来源 | 失败响应 |
|---|---|---|---|---|
| Stronghold 加密 | snapshot_encrypted 字段无明文残留 | `SELECT COUNT(*) FROM compensations WHERE snapshot_encrypted IS NULL AND reverse_payload != ''` = 0(stronghold feature 启用) | `tests/w9_stronghold_unit.rs` | 阻塞 Plan 5/6 |
| Taint Tracking | taints 表有 CRUD 代码 + gateway 查表驱动 | grep `INSERT INTO taints` / `find_by_value` 非空 | `tests/w9_taint_tracking_unit.rs` | 阻塞 Plan 7 |
| DAG Modify | Modify → 重新审批 → 执行闭环 | tauri-gated 测试通过 | `tests/w9_dag_modify_smoke.rs` | 阻塞 Plan 5/6 |
| 真实 Playwright DAG | `form.prepare` + `form.submit` 串联 | 手动运行通过 | `#[ignore]` 测试 | 不阻塞 CI |
| 真实 UIA DAG | `note.capture` + `files.organize` 串联 + Slot 流水 | 手动运行通过 | `#[ignore]` 测试 | 不阻塞 CI |
| Slot 流水闭合 | `IterableSource::UserSlot` 不返回 Err | 单元测试通过 | `tests/w9_template_unit.rs` | 阻塞 Plan 6 |
| 非门控测试数 | default `cargo test --no-default-features` | ≥ 286 | workspace test list | 阻塞 milestone |
| clippy | `-D warnings` | 0 警告 | 2 套 feature 组合 | 阻塞 milestone |
| npm build | `npm.cmd run build` | PASS | vite build | 阻塞 milestone |
| 哈希链 | audit_logs 哈希链不断 | 验收脚本验证 | `tests/w9_audit_chain_smoke.rs` | 阻塞 milestone |

---

## 6. 安全约束

### 6.1 Stronghold 密钥管理

- 密钥派生用 Argon2id(m=64MB, t=3, p=4),salt 持久化在 `config` 表
- Stronghold vault 文件存储在 `${data_dir}/stronghold.bin`,文件权限 600(Windows ACL 限制仅当前用户)
- 内存中的 key material 在 `lock()` 时清零(zeroize)
- 密码不在任何日志 / 审计 / 错误消息中出现
- 降级模式审计记录原因,UI 明确提示用户

### 6.2 Taint Tracking

- taints 表的 `value_hash` 是 SHA256(value),不存储原始值(隐私)
- `taints_json` 仅存 taint 标签,不含原始值
- gateway 拦截 taint 传播时,审计记录 taints + sink,不记录 resource 原始值
- 任务清理时 `delete_by_source` 删除关联 taints(避免无限增长)

### 6.3 DAG Modify

- Modify 后的 modified_plan 必须通过 `SlotTemplateEngine::validate_dag` 重新校验(防止 LLM 投毒或用户误操作)
- Modify 只允许一次(第二次直接 Allow/Deny),防止无限递归
- modified_plan 的 risk_ceiling 不能超过原 plan 的 max risk(用户不能通过 Modify 提权)
- `approval_request_id` 单次使用(既有规则,Modify 不破坏)

### 6.4 审计事件

| event_type | details 字段 | 隐私处理 |
|---|---|---|
| `stronghold_snapshot_encrypted` | `{compensation_id, vault_ref, plaintext_len}` | 不记录 plaintext |
| `stronghold_snapshot_decrypt_failed` | `{compensation_id, error}` | error 不含密钥 |
| `stronghold_degraded_mode_entered` | `{reason}` | reason 不含密码 |
| `taint_propagated` | `{source_ref, input_hash, output_hash, taints}` | hash 不含原始值 |
| `taint_blocked` | `{taints, sink, resource_hash}` | hash 不含原始值 |
| `dag_skeleton_modified` | `{plan_id, modified_node_count, added_count, removed_count}` | 不记录 input_template 内容(可能含敏感数据) |
| `dag_modify_limit_exceeded` | `{plan_id}` | — |

---

## 7. 已知偏离 / 延后项(W9 之后)

- **fs_snapshot 采集延后 W10+:** W9 仅加密 reverse_payload,文件系统快照采集(文件内容 / 元数据)延后 W10+
- **Cedar 引擎 taint 查询延后 W10+:** W9 仅实现 Rust 层查表驱动,Cedar policy 内嵌 taint 查询延后 W10+
- **文件系统 sink taint 传播延后 W10+:** W9 仅 Skill 输入 → 输出传播,文件系统写入 taint 标记延后 W10+
- **DAG Modify 改 loop_specs 延后 W10+:** W9 仅允许改 nodes + input_template + risk_ceiling,循环规格修改延后 W10+
- **DAG 节点并行执行延后 W10+:** W8 已延后,W9 不涉及
- **LLM 调用计费 / 速率限制延后 W10+:** W8 已延后,W9 不涉及
- **Stronghold 密钥恢复机制延后 W10+:** W9 接受"密码丢失 = 永久不可解"语义,恢复密钥(shamir secret sharing 等)延后 W10+
- **D3/E3 红色高亮:** 仍延后(自 W6b-3a 起未实现),W9 不在范围

---

## 8. 实现顺序建议(供 writing-plans 参考)

1. **Plan 1:** StrongholdVault + Argon2id 密钥派生 + config 存储 + 降级模式 + 单元测试
2. **Plan 4(与 Plan 1 并行):** DagApprovalOutcome + dag_executor Modify 分支 + TauriApprover 扩展 + UI 编辑器 + 重新审批闭环
3. **Plan 2:** create_post_commit_compensation 注入 Stronghold + reverse_compensation 解密 + 明文残留检测 + 审计事件
4. **Plan 3:** TaintRepo CRUD + dispatcher 传播 + gateway 查表驱动 + 审计事件
5. **Plan 5(与 Plan 6 并行):** 真实 Playwright MCP DAG E2E + `#[ignore]` 测试
6. **Plan 6:** Slot 流水闭合 + 真实 UIA GUI DAG E2E + `#[ignore]` 测试
7. **Plan 7:** 集成验收 + 7 套 feature cargo check + clippy + npm build + PROGRESS.md 收尾

---

## 9. 测试矩阵

| 测试文件 | Plan | feature 组合 | 类型 | 测试数(估) |
|---|---|---|---|---|
| `w9_stronghold_unit.rs` | 1 | `stronghold` | 单元 | 8 |
| `w9_dag_modify_smoke.rs` | 4 | `tauri,llm` | 集成 | 6 |
| `w9_snapshot_encrypted_smoke.rs` | 2 | `stronghold,llm` | 集成 | 5 |
| `w9_taint_tracking_unit.rs` | 3 | default | 单元 | 10 |
| `w9_gateway_taint_smoke.rs` | 3 | default | 集成 | 4 |
| `w9_plan5_playwright_dag_e2e.rs` | 5 | `voice,tauri,llm,stronghold` | `#[ignore]` | 2 |
| `w9_plan6_uia_dag_e2e.rs` | 6 | `voice,tauri,llm,uia,stronghold` | `#[ignore]` | 2 |
| `w9_default_boundary_smoke.rs` | 7 | default | 边界 | 15 |
| `w9_audit_chain_smoke.rs` | 7 | default | 集成 | 3 |
| **总计** | | | | **55+** |

W9 完成后非门控测试数预估:465(W8 收尾)+ 55(W9 新增)≈ 520(远超 ≥ 286 阈值)

---

## 10. Conventions

- **PowerShell**:`;` 分隔命令,不用 `&&` / `||`;`git commit -m "msg"` 单行(参考 project_memory.md "Lessons Learned")
- **TDD**:每个 Plan 先写失败测试 → 跑 → 实现 → 跑通 → commit
- **Feature flag 模式**:`stronghold = ["dep:tauri-plugin-stronghold", "dep:argon2"]`,参考 `uia` feature(核实报告 3.4)
- **审计事件命名**:沿用 W7/W8 的 `lower_snake_case`(如 `stronghold_snapshot_encrypted`),不用早期 SCREAMING_SNAKE
- **`#[ignore]` 真实 E2E 测试**:复用 W7 Plan 4/5 模式(核实报告 3.1-3.2),探测环境 + 短路 passing
- **TrustKernel 不是 Clone**:e2e 测试用 `Arc<TrustKernel>` 共享(参考 project_memory.md "Lessons Learned")
- **Stronghold feature 独立**:`stronghold` feature 不依赖 `voice` / `tauri` feature,可独立编译
- **commit message**:`feat(w9pN): ...` / `test(w9pN): ...` / `fix(w9pN): ...` / `docs(w9pN): ...`
- **不引入非必要依赖**:W9 仅新增 `tauri-plugin-stronghold` + `argon2`(均 workspace 级)
- **不修改 spec / 已有 plan**:若发现 spec 描述与实现不一致,记录到 PROGRESS.md "已知偏离" 段落,不回改 spec
- **空 commit 标记里程碑**:W9 收尾 `git commit --allow-empty -m "docs(w9): W9 complete — Stronghold + Taint + DAG Modify + Real E2E"`

---

## 11. 与 W8 的兼容性

| W8 行为 | W9 修改 | 兼容性 |
|---|---|---|
| `snapshot_encrypted: None` 永远 | W9 Plan 2 注入真实加密 | 向后兼容:stronghold feature 未启用时保持 None |
| `taints` 表空壳 | W9 Plan 3 实现 CRUD | 向后兼容:无既有调用代码 |
| `DagApprovalDecision::Modify` 占位 | W9 Plan 4 激活 | 向后兼容:Allow/Deny 路径不变 |
| `DagExecutor::run(plan)` 签名 | W9 Plan 6 扩展为 `run(plan, user_slots)` | **breaking change**:W8 既有调用点 + W9 新增调用点需更新(实测 44+ 处,grep `\.run\(&[a-z_]` 全 workspace;含 W8 测试 + W9 Plan 5 测试;调用点改为 `run(plan, &[])` 保持行为等价) |
| `IterableSource::UserSlot` 返回 Err | W9 Plan 6 实现 | 向后兼容:user_slots=[] 时仍可工作 |
| `dag_node_succeeded.evidence_strength = "weak"` | W9 顺手修正为真实 evidence | 向后兼容:仅审计 details 字段变化 |
| `dag_skeleton_approved.decision = format!("{:?}")` | W9 顺手修正为字符串 | 向后兼容:仅审计 details 字段变化 |

**W8 既有测试不回归保证:**

- W9 所有新增功能用 feature flag 门控(`stronghold`)
- `DagExecutor::run` 签名变更:W8 既有测试调用点改为 `run(plan, &[])`(空 user_slots,行为等价 W8)
- `DagApprovalOutcome` 新增 Modify 变体:W8 既有 AutoApprover / AutoDenier 返回 `Allow` / `Deny`,行为等价 W8

---

## 12. 用户决策记录

| 决策 # | 决策内容 | 日期 | 来源 |
|---|---|---|---|
| 1 | Stronghold 加密范围仅 reverse_payload | 2026-07-28 | 本 spec §1 |
| 2 | 密钥管理用 Argon2id + Stronghold SaltClientHash | 2026-07-28 | 本 spec §1 |
| 3 | 密码丢失 = 永久不可解,降级模式运行 | 2026-07-28 | 本 spec §1 |
| 4 | Taint Tracking 完整 CRUD + 查表驱动 gateway | 2026-07-28 | 本 spec §1 |
| 5 | Taint 传播仅 Skill 输入 → 输出 | 2026-07-28 | 本 spec §1 |
| 6 | DAG Modify 允许增删节点 + 改 input_template + risk_ceiling | 2026-07-28 | 本 spec §1 |
| 7 | Modify 后重新走完整 skeleton 审批 | 2026-07-28 | 本 spec §1 |
| 8 | 真实 E2E 测试 `#[ignore]` 手动运行 | 2026-07-28 | 本 spec §1 |
| 9 | 真实 E2E 覆盖 note.capture + files.organize + form.prepare + form.submit | 2026-07-28 | 本 spec §1 |
| 10 | Slot 流水欠债在 W9 Plan 6 闭合 | 2026-07-28 | 本 spec §1 |
| 11 | W9 合并方案 A(Stronghold + Taint)+ 方案 B(DAG Modify + 真实 E2E) | 2026-07-28 | 对话"把方案A也一起搞了呗" |
| 12 | W9 不提交,仅生成完整 plan 文档 | 2026-07-28 | 对话"先编写完整的W9 plan,以待后续完整分步执行" |

---

**End of W9 Design Spec**
