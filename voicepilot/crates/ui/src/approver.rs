//! TauriApprover —— 将同步的 `Approver::prompt` 桥接到异步的 Tauri 事件。
//!
//! V1.1 §8.2 + §6.2:Approval 窗口必须接受一次性 approval_request_id
//! 并在决定后消费。本模块实现持有待处理 approval senders 的注册表
//! + 阻塞在 recv 上的 Approver trait 实现。
//!
//! W8 Plan 5:扩展 `approve_dag_skeleton` 真实实现(emit `dag-approval-request`
//! 事件 + oneshot channel + 5min timeout → Deny),替换 Plan 2 stub。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;
use trust_kernel::approval::approver::Approver;
use trust_kernel::approval::types::ApprovalDecision;
use trust_kernel::error::Result as KernelResult;
use trust_kernel::policy::transaction::EffectManifest;
use trust_kernel::skills::dag_types::DagPlan;
use uuid::Uuid;

const DEFAULT_APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

/// Payload emitted to the webview on the `approval-request` event.
/// V1.1 §8.2:Approval 窗口必须接受一次性 approval_request_id
/// 并在决定后通过 `submit_approval` command 消费。
#[derive(serde::Serialize, Clone)]
struct ApprovalRequestPayload {
    approval_request_id: String,
    manifest: EffectManifest,
}

/// W8 Plan 5:Payload emitted to the webview on the `dag-approval-request` event.
///
/// 与 `ApprovalRequestPayload` 区别:携带完整 `DagPlan` JSON(webview 渲染
/// 节点卡片 + 边连线图)+ `user_goal` + `max_total_steps` + `node_count`。
/// webview `DagApprovalDialog` 据此显示骨架并收集 Allow/Deny 决策。
#[derive(serde::Serialize, Clone)]
pub struct DagApprovalRequestPayload {
    pub approval_request_id: String,
    pub plan_id: String,
    pub user_goal: String,
    pub max_total_steps: u32,
    /// 节点数(前端用于显示 "审批 N 个节点")
    pub node_count: usize,
    /// 完整 DagPlan JSON(webview 渲染节点卡片 + 边连线图)
    pub plan_json: serde_json::Value,
}

#[derive(Clone)]
pub struct ApprovalRegistry {
    senders: Arc<Mutex<HashMap<String, oneshot::Sender<ApprovalDecision>>>>,
}

impl ApprovalRegistry {
    pub fn new() -> Self {
        Self {
            senders: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 创建新的待处理 approval 请求。
    /// 返回 (approval_request_id, receiver) —— 调用方阻塞在 receiver 上。
    pub fn create_request(
        &self,
        _manifest: &EffectManifest,
    ) -> (String, oneshot::Receiver<ApprovalDecision>) {
        let approval_id = format!("apr_{}", Uuid::new_v4());
        let (tx, rx) = oneshot::channel::<ApprovalDecision>();
        self.senders
            .lock()
            .unwrap()
            .insert(approval_id.clone(), tx);
        (approval_id, rx)
    }

    /// 查找并移除给定 approval_request_id 的 sender。
    /// 由 `submit_approval` Tauri command 调用。
    /// 如果请求已被消费或已过期,返回 None。
    pub fn take_sender(&self, approval_id: &str) -> Option<oneshot::Sender<ApprovalDecision>> {
        self.senders.lock().unwrap().remove(approval_id)
    }

    /// 阻塞直到决定到达或超时。
    /// 超时或 sender 被丢弃时:返回 Deny(更安全的默认值)。
    pub fn wait_for_decision(
        &self,
        rx: oneshot::Receiver<ApprovalDecision>,
        timeout: Duration,
    ) -> ApprovalDecision {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("failed to build tokio runtime");
        rt.block_on(async move {
            match tokio::time::timeout(timeout, rx).await {
                Ok(Ok(decision)) => decision,
                Ok(Err(_)) => ApprovalDecision::Deny, // sender dropped
                Err(_) => ApprovalDecision::Deny,     // timeout
            }
        })
    }
}

impl Default for ApprovalRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct TauriApprover {
    registry: ApprovalRegistry,
    app: Option<AppHandle>,
}

impl TauriApprover {
    /// 测试用构造函数 —— 不发射事件,只阻塞等待决定。
    pub fn new(registry: ApprovalRegistry) -> Self {
        Self {
            registry,
            app: None,
        }
    }

    /// 生产用构造函数 —— 在 prompt 中向 webview 发射 approval-request 事件。
    pub fn with_app(registry: ApprovalRegistry, app: AppHandle) -> Self {
        Self {
            registry,
            app: Some(app),
        }
    }

    pub fn registry(&self) -> &ApprovalRegistry {
        &self.registry
    }
}

impl Approver for TauriApprover {
    fn prompt(&self, manifest: &EffectManifest) -> ApprovalDecision {
        let (approval_id, rx) = self.registry.create_request(manifest);

        // 生产环境:向 webview 发射 "approval-request" 事件。
        // 单元测试:调用方直接调用 `registry.take_sender(id).send(decision)`。
        if let Some(app) = &self.app {
            let payload = ApprovalRequestPayload {
                approval_request_id: approval_id.clone(),
                manifest: manifest.clone(),
            };
            let _ = app.emit("approval-request", payload);
        }

        self.registry.wait_for_decision(rx, DEFAULT_APPROVAL_TIMEOUT)
    }

    /// W8 Plan 5:DAG 骨架审批(委托给 inherent method)。
    ///
    /// inherent method `approve_dag_skeleton` 返回 `ApprovalDecision`,
    /// trait method 包装为 `Result<ApprovalDecision>` 以匹配 trait 签名。
    /// DagExecutor 通过 `&dyn Approver` 调用此方法,会路由到 inherent method。
    fn approve_dag_skeleton(&self, plan: &DagPlan) -> KernelResult<ApprovalDecision> {
        Ok(TauriApprover::approve_dag_skeleton(self, plan))
    }
}

// ===== W8 Plan 5: DAG 骨架审批 =====

impl TauriApprover {
    /// W8 §2.3 + §2.7:DAG 骨架审批入口。
    ///
    /// 由 `DagExecutor::run` 在拓扑排序后、节点执行前调用。
    /// 创建 oneshot channel + approval_request_id,emit `dag-approval-request`
    /// 事件给 webview,阻塞等待决策(5min timeout,默认 Deny)。
    ///
    /// 与 `prompt` 的区别:
    /// - `prompt` 用于单步 Skill 审批(payload = EffectManifest,事件 `approval-request`)
    /// - `approve_dag_skeleton` 用于 DAG 骨架审批(payload = DagPlan,事件 `dag-approval-request`)
    ///
    /// 一次性语义:`approval_request_id` 用后即焚,`take_sender` 移除 sender,
    /// 防重放(参考 project_memory.md "Tauri IPC 三安全规则")。
    pub fn approve_dag_skeleton(&self, plan: &DagPlan) -> ApprovalDecision {
        // DagPlan 不实现 EffectManifest 转换,用 dummy manifest 占位创建 oneshot channel。
        // ApprovalRegistry::create_request 的 manifest 参数仅用于日志,不影响 channel 语义。
        let dummy_manifest = EffectManifest {
            sources: vec![],
            destination: format!("dag://{}", plan.plan_id),
            conflicts: vec![],
            total_bytes: 0,
        };
        let (approval_id, rx) = self.registry.create_request(&dummy_manifest);

        if let Some(app) = &self.app {
            let plan_json = serde_json::to_value(plan).unwrap_or(serde_json::json!({}));
            let payload = DagApprovalRequestPayload {
                approval_request_id: approval_id.clone(),
                plan_id: plan.plan_id.clone(),
                user_goal: plan.user_goal.clone(),
                max_total_steps: plan.max_total_steps,
                node_count: plan.nodes.len(),
                plan_json,
            };
            let _ = app.emit("dag-approval-request", payload);
        }

        self.registry.wait_for_decision(rx, DEFAULT_APPROVAL_TIMEOUT)
    }

    /// 测试用:不 emit 事件,直接返回 approval_request_id + receiver。
    /// 单元测试调 `take_sender(id).send(decision)` 模拟用户决策。
    pub fn create_dag_approval_request_for_test(
        &self,
        plan: &DagPlan,
    ) -> (String, oneshot::Receiver<ApprovalDecision>) {
        let dummy_manifest = EffectManifest {
            sources: vec![],
            destination: format!("dag://{}", plan.plan_id),
            conflicts: vec![],
            total_bytes: 0,
        };
        self.registry.create_request(&dummy_manifest)
    }
}
