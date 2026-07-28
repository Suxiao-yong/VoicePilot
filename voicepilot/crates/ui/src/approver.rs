//! TauriApprover —— 将同步的 `Approver::prompt` 桥接到异步的 Tauri 事件。
//!
//! V1.1 §8.2 + §6.2:Approval 窗口必须接受一次性 approval_request_id
//! 并在决定后消费。本模块实现持有待处理 approval senders 的注册表
//! + 阻塞在 recv 上的 Approver trait 实现。

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

    /// W8 Plan 2 stub:DAG 骨架审批。
    ///
    /// W6 既有 `prompt` 用 oneshot channel + 5min timeout,默认 Deny。
    /// Plan 5 会实现真实 DAG 骨架弹窗(显示节点列表 + 边连线图)。
    /// 此 stub 返回 Ok(Deny) 以让 DagExecutor 的 Deny 短路逻辑可被测试,
    /// 同时避免单元测试阻塞 5min。
    fn approve_dag_skeleton(&self, _plan: &DagPlan) -> KernelResult<ApprovalDecision> {
        // Plan 5 TODO: 通过 app_handle 触发 DagApprovalDialog.vue,
        // 5min timeout → Deny;oneshot channel 接收 Allow/Deny。
        Ok(ApprovalDecision::Deny)
    }
}
