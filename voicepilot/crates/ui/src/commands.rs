//! Tauri commands —— V1.1 §8.2 webview 与 trust-kernel 之间的 IPC 桥接。
//!
//! 所有 commands 都用 `#[cfg(feature = "tauri")]` 门控。它们接收 `&AppState`
//! (由 Tauri 管理)并返回 `Result<T, String>` 供 webview 消费。

use crate::error::UiResult;
use crate::state::AppState;
use serde::{Deserialize, Serialize};

/// 镜像 `trust_kernel::voice::router_bridge::RouteOutcome`,但带 Serialize
/// 作为 Tauri command 返回类型。
///
/// W7:`Routed` 加 `slots: Vec<Slot>` 字段。LLM fallback 命中时 `SkillRouter` 返回
/// `SkillWithSlots`,此 variant 携带 LLM 提取的 Slot 列表,UI 在 Chip 区域渲染。
/// 同步 keyword 路径(`SkillRouter::route`)不提取 Slot,`slots` 为空 Vec。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RouteTextResult {
    Routed {
        skill_id: String,
        /// W7:LLM 提取的 Slot 列表(keyword 路径为空 Vec)。
        /// 前端据此渲染 Chip 修改 UI,用户确认后才提交 Skill 执行。
        slots: Vec<crate::slot_parser::Slot>,
    },
    Unmatched {
        text: String,
    },
    Empty,
    /// 无 Skill 命中时的 LLM 直接回答（聊天兜底）。前端直接展示 (+TTS)。
    Chat {
        text: String,
    },
}

/// 通过 PlannerPipeline 统一规划入口路由文本(或任意文本输入)。
///
/// W1 Task 1.3:不再自建 SkillRouter(消除 manifest 注册重复),规划完全委托给
/// `PlannerPipeline`(keyword → LLM classify → LLM DAG 拆解,纯规划、无 DB 副作用)。
/// - `Skill{extension_id, slots}` → `Routed{skill_id, slots}`,slots 由 UI 层
///   `convert_extracted_slots` 转为 UI Slot DTO(keyword 命中时为空 Vec)
/// - `Dag` → `Unmatched`(UI 尚无 DAG 审批弹窗,W8 Plan 5 实现;与既有行为一致)
/// - `Unmatched` → `Unmatched`;`Empty` → `Empty`
///
/// 用户自定义 Skill 覆盖语义由 extension catalog 保证(registry 加载 user skills
/// 时同 id 覆盖 built-in,与 SkillRouter::register 语义一致),UI 不再单独注册。
///
/// 路由阶段不执行 Skill;Skill 执行需要用户在 UI 上确认 Slot 后由 `organize_files_command` 触发。
pub async fn route_text(state: &AppState, text: &str) -> UiResult<RouteTextResult> {
    use trust_kernel::planner::{PlanResult, PlannerInput, PlannerPipeline, PlannerSource};

    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(RouteTextResult::Empty);
    }

    let pipeline = PlannerPipeline::new(state.kernel.clone(), state.kernel.extension_snapshot());
    let (plan, _trace) = pipeline
        .plan(PlannerInput {
            text: trimmed.to_string(),
            source: PlannerSource::Text,
            // 文本路径无麦克风快照：传 None，行为与旧版一致。
            snapshot: None,
        })
        .await?;

    Ok(match plan {
        PlanResult::Empty => RouteTextResult::Empty,
        // SkillWithSlots / 快路由 slots：kind 即后端 input 名，直接转 UI Slot。
        // 非 llm 构建同样转换（快路由不依赖 LLM；keyword 路径 slots 本来就是空）。
        PlanResult::Skill {
            extension_id,
            slots,
        } => RouteTextResult::Routed {
            skill_id: extension_id,
            slots: crate::slot_parser::convert_extracted_slots(&slots),
        },
        // UI 尚无 DAG 审批弹窗:与既有行为一致,防御性映射为 Unmatched。
        PlanResult::Dag(_) => RouteTextResult::Unmatched {
            text: trimmed.to_string(),
        },
        // 聊天兜底：透传展示。
        PlanResult::Chat { text } => RouteTextResult::Chat { text },
        PlanResult::Unmatched { text } => RouteTextResult::Unmatched { text },
    })
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn route_text_command(
    state: tauri::State<'_, AppState>,
    text: String,
) -> Result<RouteTextResult, String> {
    route_text(&state, &text).await.map_err(Into::into)
}

// ===== files.organize Skill command (V1.1 §5.2 + §6.2 + §8.2) =====

use std::path::PathBuf;
use trust_kernel::approval::approver::Approver;
use trust_kernel::repo::step_repo::StepRecord;
use trust_kernel::skills::executor::{FilesOrganizeInput, FilesOrganizeSkill};
use trust_kernel::toolresult::ToolStatus;

/// Webview → Tauri 入参,镜像 `FilesOrganizeInput` 但用 String 路径
/// (serde 友好,跨 IPC 边界无 PathBuf 序列化问题)。
#[derive(Debug, Clone, Deserialize)]
pub struct OrganizeInput {
    pub task_id: String,
    pub step_id: String,
    pub source: String,
    pub filter: String,
    pub destination: String,
}

/// `organize_files` 返回值 —— 桥接 SkillExecution 的 ToolResult V2 字段
/// 到 webview 可消费的扁平结构。
#[derive(Debug, Clone, Serialize)]
pub struct OrganizeResult {
    pub committed: bool,
    pub moved_paths: Vec<[String; 2]>,
    pub evidence_strength: String,
    pub compensation_ref: Option<String>,
    pub error: Option<String>,
}

/// 编排 files.organize Skill 完整管道:
/// prepare → approve → commit → verify → compensate(V1.1 §6.2)。
///
/// 注入的 `approver` 决定 approve 阶段行为:
/// - 测试用 `AutoApprover`(无条件 Allow)
/// - 生产用 `TauriApprover`(通过 IPC 等待 Approval 窗口决定)
///
/// 幂等创建 task/step:若 task_id / step_id 在 kernel 中不存在,则创建
/// (Skill executor 内部第一步就调用 `update_step_status`,要求 step 已存在)。
pub fn organize_files(
    state: &AppState,
    approver: &dyn Approver,
    input: &OrganizeInput,
) -> UiResult<OrganizeResult> {
    let kernel = state.kernel.clone();

    // Idempotent task creation.
    if kernel.get_task(&input.task_id)?.is_none() {
        kernel.create_task(&input.task_id, "files.organize")?;
    }

    // Idempotent step creation.
    if kernel.get_step(&input.step_id)?.is_none() {
        let step = StepRecord::new(input.step_id.clone(), input.task_id.clone(), 1);
        kernel.create_step(&step)?;
    }

    let skill_input = FilesOrganizeInput {
        task_id: input.task_id.clone(),
        step_id: input.step_id.clone(),
        source: PathBuf::from(&input.source),
        filter: input.filter.clone(),
        destination: PathBuf::from(&input.destination),
    };

    let skill = FilesOrganizeSkill::new();
    let execution = skill.execute(kernel.as_ref(), &skill_input, approver)?;

    let moved_paths = execution
        .moved_paths
        .into_iter()
        .map(|(from, to)| {
            [
                from.to_string_lossy().into_owned(),
                to.to_string_lossy().into_owned(),
            ]
        })
        .collect();

    Ok(OrganizeResult {
        committed: execution.tool_result.status == ToolStatus::Succeeded,
        moved_paths,
        evidence_strength: execution.tool_result.evidence_strength.as_str().to_string(),
        compensation_ref: execution.tool_result.compensation_ref.clone(),
        error: None,
    })
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn organize_files_command(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    input: OrganizeInput,
) -> Result<OrganizeResult, String> {
    let approver = crate::approver::TauriApprover::with_app(state.approval_registry.clone(), app);
    organize_files(&state, &approver, &input).map_err(Into::into)
}

// ===== execute_skill Skill 通用执行命令（Skill 执行接线 Phase 1） =====

/// Webview → Tauri 入参：调用方提供 task/step id（`ui-${timestamp}` / `s-${timestamp}`，
/// 沿用 PetWindow 模式）+ 目标 skill + 已 resolve 的槽位 JSON。
#[derive(Debug, Clone, Deserialize)]
pub struct ExecuteSkillInput {
    pub task_id: String,
    pub step_id: String,
    pub skill_id: String,
    pub slots_json: serde_json::Value,
}

/// 返回值：executor 级失败（拒绝 / 校验 / 无 adapter）走 `error` 字段，
/// 前端据此渲染成功 / 拒绝 / 失败三种态；只有 IPC 级故障才走 `Err`。
#[derive(Debug, Clone, Serialize)]
pub struct ExecuteSkillResult {
    pub committed: bool,
    pub summary: String,
    pub error: Option<String>,
}

/// 通用 Skill 执行：直接走 `dispatch_skill_executor`。
///
/// 约定（沿 DagExecutor 先例，不沿 organize 预建模式）：
/// - task/step 由各 executor 自己创建，此处不预建（预建会与
///   `execute_app_control` 的 `create_task` 主键冲突，见计划附录 Phase 0）。
/// - UIA Skill 从线程本地取 adapter：生产命令层注入真实 adapter，
///   测试注入 mock；缺失时 dispatch 返回可读错误，不 panic。
pub fn execute_skill(
    state: &AppState,
    approver: &dyn Approver,
    input: &ExecuteSkillInput,
) -> UiResult<ExecuteSkillResult> {
    use trust_kernel::skills::dispatcher::dispatch_skill_executor;

    match dispatch_skill_executor(
        &input.skill_id,
        &state.kernel,
        &input.slots_json,
        approver,
        &input.task_id,
        &input.step_id,
    ) {
        Ok(outcome) => Ok(ExecuteSkillResult {
            committed: outcome.succeeded,
            summary: if outcome.succeeded {
                format!("{} 执行成功", input.skill_id)
            } else {
                String::new()
            },
            error: outcome.error_cause,
        }),
        Err(e) => Ok(ExecuteSkillResult {
            committed: false,
            summary: String::new(),
            error: Some(e.to_string()),
        }),
    }
}

/// Skills whose executors drive Windows UI (thread-local adapter).
/// Everything else must NOT pay adapter-construction cost or fail when the
/// `mcp-windows` backend is missing/disabled — e.g. files.organize works
/// with no UI backend at all. Keep in sync with the dispatch arms that
/// actually call `thread_local_uia_adapter()`.
pub fn needs_uia_adapter(skill_id: &str) -> bool {
    matches!(skill_id, "quick.app_control" | "note.capture")
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn execute_skill_command(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    input: ExecuteSkillInput,
) -> Result<ExecuteSkillResult, String> {
    let approver = crate::approver::TauriApprover::with_app(state.approval_registry.clone(), app);
    // dispatch_app_control / dispatch_note_capture 从线程本地取 UIA adapter。
    // 此同步段内无 `.await`，set → dispatch → clear 同线程。
    // 后端是外部 mcp-windows 子进程（`mcp_servers` 表 `mcp-windows` 行，
    // 缺失/禁用/无 exe 时构造即返回可读错误，不起进程）。
    // Drop guard 清 None：tokio worker 线程复用，dispatch panic 时也不泄漏到后续命令。
    // Non-UIA skills skip the adapter entirely: constructing it would fail
    // the whole command when the mcp-windows backend is missing/disabled,
    // even though e.g. files.organize needs no UI at all (P0).
    #[cfg(all(windows, feature = "uia"))]
    {
        if !needs_uia_adapter(&input.skill_id) {
            return execute_skill(&state, &approver, &input).map_err(Into::into);
        }
        struct ClearUiaAdapterOnDrop;
        impl Drop for ClearUiaAdapterOnDrop {
            fn drop(&mut self) {
                trust_kernel::skills::dag_executor::set_thread_local_uia_adapter(None);
            }
        }
        let adapter =
            trust_kernel::uiautomation::adapter::McpUiaAdapter::windows_default(&state.kernel)
                .map_err(|e| format!("UIA adapter 初始化失败: {e}"))?;
        trust_kernel::skills::dag_executor::set_thread_local_uia_adapter(Some(
            std::sync::Arc::new(adapter),
        ));
        let _clear_on_drop = ClearUiaAdapterOnDrop;
        execute_skill(&state, &approver, &input).map_err(Into::into)
    }
    #[cfg(not(all(windows, feature = "uia")))]
    {
        execute_skill(&state, &approver, &input).map_err(Into::into)
    }
}

/// 在 `commands.rs` 内部调用 `tauri::generate_handler!`,使 `#[tauri::command]`
/// 生成的 `__tauri_command_name_*` / `__cmd__*` 辅助宏在本模块作用域内可见。
/// 在 `app.rs` 中跨模块调用 `generate_handler!` 会因为宏作用域问题报
/// "cannot find macro" 错误。
///
/// 单态化到 `Wry` 运行时:`tauri::AppHandle`(= `AppHandle<Wry>`)只实现
/// `CommandArg<'_, Wry>`,若 `R` 仍是泛型,闭包类型推断无法满足 trait bound。
#[cfg(feature = "tauri")]
pub fn register_handlers(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.invoke_handler(tauri::generate_handler![
        route_text_command,
        organize_files_command,
        execute_skill_command,
        submit_approval_command,
        submit_clarification_command,
        crate::settings_commands::get_settings_command,
        crate::settings_commands::update_settings_command,
        crate::settings_commands::test_llm_command,
        crate::audit_commands::list_audit_recent_command,
        crate::model_download_commands::download_model_command,
        // W8 Plan 5: DAG 相关命令
        crate::dag_commands::approve_dag_skeleton_command,
        crate::dag_commands::list_dag_history_command,
        crate::dag_commands::get_dag_plan_command,
        crate::dag_commands::get_task_explanation_command,
        // 桌宠化改造:窗口显隐 / 唯一退出路径 / 气泡可见性同步
        crate::pet_commands::show_main_window,
        crate::pet_commands::hide_main_window,
        crate::pet_commands::exit_app,
        crate::pet_commands::pet_set_bubble_visible,
        crate::pet_commands::pet_probe,
    ])
}

/// Voice feature on 时的 handler 注册(包含 voice_listen_command)。
///
/// Tauri 2 的 `invoke_handler` 是替换语义(不是追加),所以需要单独的函数
/// 把 voice_listen_command 加入 `generate_handler!` 列表。
#[cfg(feature = "voice")]
pub fn register_handlers_with_voice(
    builder: tauri::Builder<tauri::Wry>,
) -> tauri::Builder<tauri::Wry> {
    builder.invoke_handler(tauri::generate_handler![
        route_text_command,
        organize_files_command,
        execute_skill_command,
        submit_approval_command,
        submit_clarification_command,
        crate::settings_commands::get_settings_command,
        crate::settings_commands::update_settings_command,
        crate::settings_commands::test_llm_command,
        crate::audit_commands::list_audit_recent_command,
        crate::audit_commands::list_audit_for_task_command,
        crate::trust_center_commands::list_mcp_servers_command,
        crate::trust_center_commands::toggle_mcp_server_command,
        crate::trust_center_commands::register_mcp_server_command,
        crate::trust_center_commands::remove_mcp_server_command,
        crate::trust_center_commands::import_mcp_servers_command,
        crate::trust_center_commands::export_mcp_servers_command,
        crate::trust_center_commands::scan_external_mcp_command,
        crate::trust_center_commands::import_external_mcp_command,
        crate::skills_commands::list_skills_command,
        crate::skills_commands::toggle_skill_command,
        crate::skills_commands::reload_skills_command,
        crate::skills_commands::import_skill_command,
        crate::skills_commands::scan_external_skills_command,
        crate::skills_commands::import_external_skill_command,
        crate::skills_commands::list_user_skills_command,
        crate::diff_commands::compute_diff_command,
        crate::voice_commands::voice_listen_command,
        crate::voice_commands::cancel_voice_command,
        crate::voice_commands::tts_command,
        crate::voice_commands::cancel_tts_command,
        crate::model_download_commands::is_voice_enabled_command,
        crate::model_download_commands::check_model_command,
        crate::model_download_commands::download_model_command,
        // W8 Plan 5: DAG 相关命令
        crate::dag_commands::approve_dag_skeleton_command,
        crate::dag_commands::list_dag_history_command,
        crate::dag_commands::get_dag_plan_command,
        crate::dag_commands::get_task_explanation_command,
        // Phase C: 后台定时作业命令
        crate::scheduler_commands::list_agent_jobs_command,
        crate::scheduler_commands::list_job_runs_command,
        crate::scheduler_commands::disable_agent_job_command,
        // 桌宠化改造:窗口显隐 / 唯一退出路径 / 气泡可见性同步
        crate::pet_commands::show_main_window,
        crate::pet_commands::hide_main_window,
        crate::pet_commands::exit_app,
        crate::pet_commands::pet_set_bubble_visible,
        crate::pet_commands::pet_probe,
    ])
}

// ===== submit_approval command (V1.1 §8.2 one-shot decision delivery) =====

use trust_kernel::approval::types::ApprovalDecision;

/// 为待处理请求提交用户的 approval 决定。
/// 如果决定已投递返回 true,如果请求已被消费或从未存在返回 false
/// (一次性,符合 §8.2)。
pub fn submit_approval(
    state: &AppState,
    approval_id: &str,
    decision: ApprovalDecision,
) -> UiResult<bool> {
    let sender = match state.approval_registry.take_sender(approval_id) {
        Some(s) => s,
        None => return Ok(false),
    };
    let _ = sender.send(decision);
    Ok(true)
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn submit_approval_command(
    state: tauri::State<'_, AppState>,
    approval_id: String,
    decision: ApprovalDecision,
) -> Result<bool, String> {
    submit_approval(&state, &approval_id, decision).map_err(Into::into)
}

// ===== 追问卡（与审批语义分离：点选下标，超时回 default_index） =====

/// 为待处理追问卡提交用户点选的 option 下标（一次性，原样透传）。
/// 后端在投递时不知道 options 长度，不做钳制；调用方取到下标后负责钳制
///（如 `media_ops::resolve_search_choice` 的 `.min(titles.len() - 1)`）。
pub fn submit_clarification(
    state: &AppState,
    clarification_id: &str,
    selected_index: usize,
) -> UiResult<bool> {
    let sender = match state
        .approval_registry
        .take_clarify_sender(clarification_id)
    {
        Some(s) => s,
        None => return Ok(false),
    };
    let _ = sender.send(selected_index);
    Ok(true)
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn submit_clarification_command(
    state: tauri::State<'_, AppState>,
    clarification_id: String,
    selected_index: usize,
) -> Result<bool, String> {
    submit_clarification(&state, &clarification_id, selected_index).map_err(Into::into)
}
