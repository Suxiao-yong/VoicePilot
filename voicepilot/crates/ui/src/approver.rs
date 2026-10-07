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

use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;
use trust_kernel::approval::approver::{Approver, DagApprovalOutcome};
use trust_kernel::approval::types::ApprovalDecision;
use trust_kernel::error::Result as KernelResult;
use trust_kernel::policy::transaction::EffectManifest;
use trust_kernel::skills::dag_types::DagPlan;
use uuid::Uuid;

const DEFAULT_APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

/// 桌宠化改造:审批事件发射前唤醒隐藏的主窗口。
///
/// 双窗口架构下 main 常处于隐藏驻留态;高风险操作审批若只在后台 emit,
/// 用户看不到 ApprovalModal 会一直阻塞到 300s 超时默认 Deny。show 对
/// 可见窗口是无操作,set_focus 抢焦点正是审批场景所需。
fn wake_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}

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

/// Payload emitted to the webview on the `clarification-request` event.
///
/// 与审批语义分离：前端渲染追问卡（三选一），超时/关闭回 `default_index`。
#[derive(serde::Serialize, Clone)]
pub struct ClarificationRequestPayload {
    pub clarification_request_id: String,
    pub question: String,
    pub options: Vec<String>,
    pub default_index: usize,
}

/// W9 Plan 4:携带 modified_plan 的 DAG 审批决策 payload。
///
/// 通过 oneshot channel 从 `submit_dag_skeleton_approval` 命令投递到
/// `TauriApprover::approve_dag_skeleton`(阻塞等待中)。
/// `decision = Modify` 时 `modified_plan` 必须为 `Some`,否则 TauriApprover
/// 返回 `Err(KernelError::Approval("Modify without modified_plan"))`。
// W9 修复(P1-14):仅 Serialize(从 UI 后端投递到 TauriApprover,无需 Deserialize)
#[derive(Debug, Clone, serde::Serialize)]
pub struct DagApprovalPayload {
    pub decision: ApprovalDecision,
    pub modified_plan: Option<DagPlan>,
}

/// 新线程里跑完 future 并 join 取回（`trust_kernel::planner::block_on_planner` 同构）。
/// 三个 wait 的统一底座：调用线程无论在 Tauri 命令协程内、setup 内还是普通
/// 测试内都安全 —— 直接 `Runtime::block_on` 在已有 runtime 上下文会 panic。
fn block_on_wait<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("failed to build tokio runtime");
        rt.block_on(future)
    })
    .join()
    .expect("wait thread panicked")
}

#[derive(Clone)]
pub struct ApprovalRegistry {
    senders: Arc<Mutex<HashMap<String, oneshot::Sender<ApprovalDecision>>>>,
    /// W9 Plan 4:DAG 骨架审批专用 senders(与 `senders` 平行,不破坏既有 `prompt` 语义)。
    dag_senders: Arc<Mutex<HashMap<String, oneshot::Sender<DagApprovalPayload>>>>,
    /// 追问卡专用 senders(与审批语义分离:超时回 default_index,不是 Deny)。
    clarify_senders: Arc<Mutex<HashMap<String, oneshot::Sender<usize>>>>,
}

impl ApprovalRegistry {
    pub fn new() -> Self {
        Self {
            senders: Arc::new(Mutex::new(HashMap::new())),
            dag_senders: Arc::new(Mutex::new(HashMap::new())),
            clarify_senders: Arc::new(Mutex::new(HashMap::new())),
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
        self.senders.lock().unwrap().insert(approval_id.clone(), tx);
        (approval_id, rx)
    }

    /// 查找并移除给定 approval_request_id 的 sender。
    /// 由 `submit_approval` Tauri command 调用。
    /// 如果请求已被消费或已过期,返回 None。
    pub fn take_sender(&self, approval_id: &str) -> Option<oneshot::Sender<ApprovalDecision>> {
        self.senders.lock().unwrap().remove(approval_id)
    }

    /// W9 Plan 4:创建 DAG 骨架审批请求(专用 oneshot channel)。
    ///
    /// 与 `create_request` 区别:
    /// - 用 `dag_xxx` 前缀的 approval_request_id(与 `apr_xxx` 区分)
    /// - 投递 `DagApprovalPayload`(含 modified_plan)而非 `ApprovalDecision`
    // W9 修复(P1-15):删除未使用的 plan 参数(原 plan 仅用于日志占位,不影响 channel 语义)
    pub fn create_dag_request(&self) -> (String, oneshot::Receiver<DagApprovalPayload>) {
        let approval_id = format!("dag_{}", Uuid::new_v4());
        let (tx, rx) = oneshot::channel::<DagApprovalPayload>();
        self.dag_senders
            .lock()
            .unwrap()
            .insert(approval_id.clone(), tx);
        (approval_id, rx)
    }

    /// W9 Plan 4:取出 DAG 骨架审批的 sender(由 `submit_dag_skeleton_approval` 调用)。
    pub fn take_dag_sender(
        &self,
        approval_id: &str,
    ) -> Option<oneshot::Sender<DagApprovalPayload>> {
        self.dag_senders.lock().unwrap().remove(approval_id)
    }

    /// W9 Plan 4:阻塞等待 DAG 审批决策,超时或 sender dropped 返回 Deny(默认安全)。
    ///
    /// 新线程 + join 取回（`trust_kernel::planner::block_on_planner` 同构）：
    /// Tauri 命令协程内直接 `block_on` 会 panic（"Cannot start a runtime
    /// from within a runtime"），自带线程则 setup/命令/测试处处可调。
    /// W9 的 `try_current` 分支已删（该分支在 multi-thread worker 内同样 panic）。
    pub fn wait_for_dag_decision(
        &self,
        rx: oneshot::Receiver<DagApprovalPayload>,
        timeout: Duration,
    ) -> DagApprovalPayload {
        block_on_wait(async move {
            match tokio::time::timeout(timeout, rx).await {
                Ok(Ok(payload)) => payload,
                Ok(Err(_)) => DagApprovalPayload {
                    decision: ApprovalDecision::Deny,
                    modified_plan: None,
                }, // sender dropped
                Err(_) => DagApprovalPayload {
                    decision: ApprovalDecision::Deny,
                    modified_plan: None,
                }, // timeout
            }
        })
    }

    /// 阻塞直到决定到达或超时。
    /// 超时或 sender 被丢弃时:返回 Deny(更安全的默认值)。
    /// 线程模型同 `wait_for_dag_decision`（新线程 + join，不依赖调用方上下文）。
    pub fn wait_for_decision(
        &self,
        rx: oneshot::Receiver<ApprovalDecision>,
        timeout: Duration,
    ) -> ApprovalDecision {
        block_on_wait(async move {
            match tokio::time::timeout(timeout, rx).await {
                Ok(Ok(decision)) => decision,
                Ok(Err(_)) => ApprovalDecision::Deny, // sender dropped
                Err(_) => ApprovalDecision::Deny,     // timeout
            }
        })
    }

    /// 创建追问卡请求。返回 (clarification_request_id, receiver)。
    pub fn create_clarify_request(&self) -> (String, oneshot::Receiver<usize>) {
        let id = format!("clf_{}", Uuid::new_v4());
        let (tx, rx) = oneshot::channel::<usize>();
        self.clarify_senders.lock().unwrap().insert(id.clone(), tx);
        (id, rx)
    }

    /// 取出追问卡的 sender(由 `submit_clarification` 调用，一次性)。
    pub fn take_clarify_sender(&self, id: &str) -> Option<oneshot::Sender<usize>> {
        self.clarify_senders.lock().unwrap().remove(id)
    }

    /// 阻塞直到用户点选或超时。超时/sender 丢弃 → 返回 default_index（不是 Deny）。
    /// 线程模型同 `wait_for_dag_decision`（新线程 + join，不依赖调用方上下文）。
    pub fn wait_for_clarification(
        &self,
        rx: oneshot::Receiver<usize>,
        timeout: Duration,
        default_index: usize,
    ) -> usize {
        block_on_wait(async move {
            match tokio::time::timeout(timeout, rx).await {
                Ok(Ok(i)) => i,
                Ok(Err(_)) => default_index, // sender dropped
                Err(_) => default_index,     // timeout
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
    /// W9 Plan 4:最近一次 create_dag_request 生成的 approval_id(测试协调用)。
    latest_dag_approval_id: Arc<Mutex<Option<String>>>,
}

impl TauriApprover {
    /// 测试用构造函数 —— 不发射事件,只阻塞等待决定。
    pub fn new(registry: ApprovalRegistry) -> Self {
        Self {
            registry,
            app: None,
            latest_dag_approval_id: Arc::new(Mutex::new(None)),
        }
    }

    /// 生产用构造函数 —— 在 prompt 中向 webview 发射 approval-request 事件。
    pub fn with_app(registry: ApprovalRegistry, app: AppHandle) -> Self {
        Self {
            registry,
            app: Some(app),
            latest_dag_approval_id: Arc::new(Mutex::new(None)),
        }
    }

    pub fn registry(&self) -> &ApprovalRegistry {
        &self.registry
    }

    /// W9 Plan 4 测试 hook:返回最近一次 create_dag_request 生成的 approval_id。
    #[cfg(test)]
    pub fn latest_approval_id(&self) -> Option<String> {
        self.latest_dag_approval_id.lock().unwrap().clone()
    }
}

impl Approver for TauriApprover {
    fn prompt(&self, manifest: &EffectManifest) -> ApprovalDecision {
        let (approval_id, rx) = self.registry.create_request(manifest);

        // 生产环境:向 webview 发射 "approval-request" 事件。
        // 单元测试:调用方直接调用 `registry.take_sender(id).send(decision)`。
        if let Some(app) = &self.app {
            wake_main_window(app); // main 隐藏驻留时先唤醒,否则审批无人可见
            let payload = ApprovalRequestPayload {
                approval_request_id: approval_id.clone(),
                manifest: manifest.clone(),
            };
            let _ = app.emit("approval-request", payload);
        }

        self.registry
            .wait_for_decision(rx, DEFAULT_APPROVAL_TIMEOUT)
    }

    /// W9 Plan 4:DAG 骨架审批入口(委托给 inherent `approve_dag_skeleton_outcome`)。
    ///
    /// 完整 IPC 实现:emit `dag-approval-request` 事件 + oneshot channel +
    /// 5min timeout → Deny + 接收 `modified_plan: Option<DagPlan>` payload,
    /// 构造 `DagApprovalOutcome::Modify`。
    fn approve_dag_skeleton(&self, plan: &DagPlan) -> KernelResult<DagApprovalOutcome> {
        TauriApprover::approve_dag_skeleton_outcome(self, plan)
    }

    /// 追问卡入口:emit `clarification-request` 事件 + oneshot channel +
    /// 5min timeout → default_index。无 app（单测）时直接回 default。
    fn request_clarification(
        &self,
        question: &str,
        options: &[String],
        default_index: usize,
    ) -> usize {
        let (id, rx) = self.registry.create_clarify_request();
        if let Some(app) = &self.app {
            wake_main_window(app);
            let payload = ClarificationRequestPayload {
                clarification_request_id: id,
                question: question.to_string(),
                options: options.to_vec(),
                default_index,
            };
            let _ = app.emit("clarification-request", payload);
        }
        self.registry
            .wait_for_clarification(rx, DEFAULT_APPROVAL_TIMEOUT, default_index)
    }
}

// ===== W8 Plan 5: DAG 骨架审批 =====

impl TauriApprover {
    /// W9 Plan 4:DAG 骨架审批入口(实现 `Approver::approve_dag_skeleton` 逻辑)。
    ///
    /// 创建 oneshot channel + approval_request_id,emit `dag-approval-request`
    /// 事件给 webview,阻塞等待决策(5min timeout,默认 Deny)。
    /// 返回 `DagApprovalOutcome`(含 Modify payload)。
    ///
    /// 与 `prompt` 的区别:
    /// - `prompt` 用于单步 Skill 审批(payload = EffectManifest,事件 `approval-request`)
    /// - `approve_dag_skeleton_outcome` 用于 DAG 骨架审批(payload = DagPlan,事件 `dag-approval-request`)
    ///
    /// 一次性语义:`approval_request_id` 用后即焚,`take_dag_sender` 移除 sender,
    /// 防重放(参考 project_memory.md "Tauri IPC 三安全规则")。
    pub fn approve_dag_skeleton_outcome(&self, plan: &DagPlan) -> KernelResult<DagApprovalOutcome> {
        use trust_kernel::error::KernelError;

        let (approval_id, rx) = self.registry.create_dag_request();
        // W9 Plan 4:记录 approval_id 供测试 hook 协调
        *self.latest_dag_approval_id.lock().unwrap() = Some(approval_id.clone());

        if let Some(app) = &self.app {
            wake_main_window(app); // main 隐藏驻留时先唤醒,否则审批无人可见
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

        let dag_payload = self
            .registry
            .wait_for_dag_decision(rx, DEFAULT_APPROVAL_TIMEOUT);
        match dag_payload.decision {
            ApprovalDecision::Allow => Ok(DagApprovalOutcome::Allow),
            ApprovalDecision::Deny => Ok(DagApprovalOutcome::Deny),
            ApprovalDecision::Modify => {
                let modified_plan = dag_payload
                    .modified_plan
                    .ok_or_else(|| KernelError::Approval("Modify without modified_plan".into()))?;
                Ok(DagApprovalOutcome::Modify {
                    modified_plan: Box::new(modified_plan),
                })
            }
        }
    }

    /// W9 Plan 4 测试用:不 emit 事件,直接返回 approval_request_id + receiver。
    /// 单元测试调 `take_dag_sender(id).send(DagApprovalPayload { decision, modified_plan })` 模拟用户决策。
    // W9 修复(P1-15):删除未使用的 plan 参数(委托 create_dag_request,后者已删 plan 参数)
    pub fn create_dag_approval_request_for_test(
        &self,
    ) -> (String, oneshot::Receiver<DagApprovalPayload>) {
        self.registry.create_dag_request()
    }
}
