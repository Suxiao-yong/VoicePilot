#![cfg(feature = "tauri")]

use voicepilot_ui::commands::RouteTextResult;
use voicepilot_ui::state::AppState;

/// W7: route_text 改为 async fn(LLM fallback 路径需 .await),
/// 测试用 `#[tokio::test]` + `.await`。
#[tokio::test]
async fn route_text_returns_routed_when_skill_keyword_matches() {
    let state = AppState::new_in_memory().unwrap();
    // `files_organize_manifest` 的 keywords 是中文("整理"、"归档"、
    // "移动文件"、"下载目录"),所以测试输入也用中文才能触发匹配。
    // keyword 命中 → LLM fallback 不触发,slots 为空 Vec。
    let result = voicepilot_ui::commands::route_text(&state, "整理下载目录")
        .await
        .unwrap();
    assert!(matches!(
        result,
        RouteTextResult::Routed { ref skill_id, .. } if skill_id == "files.organize"
    ));
}

#[tokio::test]
async fn route_text_returns_unmatched_when_no_keyword() {
    let state = AppState::new_in_memory().unwrap();
    let result = voicepilot_ui::commands::route_text(&state, "hello world")
        .await
        .unwrap();
    assert!(matches!(result, RouteTextResult::Unmatched { .. }));
}

#[tokio::test]
async fn route_text_returns_empty_for_whitespace() {
    let state = AppState::new_in_memory().unwrap();
    let result = voicepilot_ui::commands::route_text(&state, "   ")
        .await
        .unwrap();
    assert!(matches!(result, RouteTextResult::Empty));
}

use tempfile::TempDir;
use trust_kernel::approval::approver::AutoApprover;
use voicepilot_ui::commands::{OrganizeInput, organize_files};

#[test]
fn organize_files_with_auto_approver_commits_move() {
    let tmp = TempDir::new().unwrap();
    let src_dir = tmp.path().join("src");
    let dest_dir = tmp.path().join("dest");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::create_dir_all(&dest_dir).unwrap();
    std::fs::write(src_dir.join("a.txt"), "hello").unwrap();

    let state = AppState::new_in_memory().unwrap();
    let result = organize_files(
        &state,
        &AutoApprover,
        &OrganizeInput {
            task_id: "t-test".to_string(),
            step_id: "s-test".to_string(),
            source: src_dir.to_string_lossy().into_owned(),
            filter: "*.txt".to_string(),
            destination: dest_dir.to_string_lossy().into_owned(),
        },
    )
    .unwrap();

    assert!(result.committed);
    assert_eq!(result.moved_paths.len(), 1);
    assert!(dest_dir.join("a.txt").exists());
}

use trust_kernel::approval::types::ApprovalDecision;
use trust_kernel::policy::transaction::EffectManifest;
use voicepilot_ui::commands::submit_approval;

#[test]
fn submit_approval_delivers_decision_to_waiting_approver() {
    let state = AppState::new_in_memory().unwrap();
    let manifest = EffectManifest {
        sources: vec![],
        destination: "D:/test".to_string(),
        conflicts: vec![],
        total_bytes: 0,
    };
    let (approval_id, rx) = state.approval_registry.create_request(&manifest);

    // 派生一个线程等待决定
    let registry = state.approval_registry.clone();
    let handle = std::thread::spawn(move || {
        registry.wait_for_decision(rx, std::time::Duration::from_secs(5))
    });

    // 给线程一点时间开始等待
    std::thread::sleep(std::time::Duration::from_millis(100));

    // 提交 approval 决定
    let result = submit_approval(&state, &approval_id, ApprovalDecision::Allow).unwrap();
    assert!(result);

    let decision = handle.join().unwrap();
    assert_eq!(decision, ApprovalDecision::Allow);
}

#[test]
fn submit_approval_returns_false_for_unknown_id() {
    let state = AppState::new_in_memory().unwrap();
    let result = submit_approval(&state, "apr_nonexistent", ApprovalDecision::Deny).unwrap();
    assert!(!result);
}

// ===== W8 Plan 5 Task 1: DAG 骨架审批 =====

use voicepilot_ui::approver::ApprovalRegistry;
use voicepilot_ui::dag_commands::{DagApprovalDecision, submit_dag_skeleton_approval};

#[test]
fn submit_dag_skeleton_approval_delivers_decision() {
    let state = AppState::new_in_memory().unwrap();
    let _registry = ApprovalRegistry::new();
    // W9 Plan 4:DAG 审批必须用 create_dag_request(生成 dag_xxx 前缀)。
    let (approval_id2, _rx2) = state.approval_registry.create_dag_request();
    let delivered =
        submit_dag_skeleton_approval(&state, &approval_id2, DagApprovalDecision::Allow, None)
            .unwrap();
    assert!(delivered, "first submission should succeed");
}

/// Wave 5 Task 5.1:Modify 审批携带修改后的 DagPlan(Some)也能投递。
#[test]
fn submit_dag_skeleton_approval_delivers_modified_plan() {
    let state = AppState::new_in_memory().unwrap();
    let (approval_id, _rx) = state.approval_registry.create_dag_request();
    let modified_plan = trust_kernel::skills::dag_types::DagPlan {
        plan_id: "modified-plan-1".into(),
        user_goal: "modify test".into(),
        nodes: vec![],
        edges: vec![],
        loop_specs: std::collections::HashMap::new(),
        max_total_steps: 1,
    };
    let delivered = submit_dag_skeleton_approval(
        &state,
        &approval_id,
        DagApprovalDecision::Modify,
        Some(modified_plan),
    )
    .unwrap();
    assert!(delivered, "Modify with Some(modified_plan) should deliver");
}

#[test]
fn submit_dag_skeleton_approval_rejects_replay() {
    // 一次性语义:同一 approval_request_id 第二次调用返回 false
    let state = AppState::new_in_memory().unwrap();
    let (approval_id, _rx) = state.approval_registry.create_dag_request();

    let first = submit_dag_skeleton_approval(&state, &approval_id, DagApprovalDecision::Deny, None)
        .unwrap();
    assert!(first, "first call should deliver");

    let second =
        submit_dag_skeleton_approval(&state, &approval_id, DagApprovalDecision::Allow, None)
            .unwrap();
    assert!(!second, "replay should be rejected (single-use)");
}

#[test]
fn submit_dag_skeleton_approval_unknown_id_returns_false() {
    let state = AppState::new_in_memory().unwrap();
    let result =
        submit_dag_skeleton_approval(&state, "dag_nonexistent", DagApprovalDecision::Allow, None)
            .unwrap();
    assert!(!result, "unknown approval_request_id should return false");
}

#[test]
fn dag_approval_decision_convert_to_approval_decision() {
    assert_eq!(
        ApprovalDecision::from(DagApprovalDecision::Allow),
        ApprovalDecision::Allow
    );
    assert_eq!(
        ApprovalDecision::from(DagApprovalDecision::Deny),
        ApprovalDecision::Deny
    );
    assert_eq!(
        ApprovalDecision::from(DagApprovalDecision::Modify),
        ApprovalDecision::Modify
    );
}

// ===== execute_skill（Skill 执行接线 Phase 1） =====

use voicepilot_ui::commands::{ExecuteSkillInput, execute_skill};

/// 无 adapter 时返回可读错误，不 panic。
/// - uia 构建：dispatch_app_control 报 thread-local 缺失
/// - 非 uia 构建：quick.app_control 不在分发映射里，snapshot 门直接拒绝
#[test]
fn execute_skill_without_adapter_returns_readable_error() {
    let state = AppState::new_in_memory().unwrap();
    #[cfg(all(windows, feature = "uia"))]
    trust_kernel::skills::dag_executor::set_thread_local_uia_adapter(None);
    let result = execute_skill(
        &state,
        &AutoApprover,
        &ExecuteSkillInput {
            task_id: "ui-1".to_string(),
            step_id: "s-1".to_string(),
            skill_id: "quick.app_control".to_string(),
            slots_json: serde_json::json!({"action": "launch", "app_name": "notepad"}),
        },
    )
    .unwrap();
    assert!(!result.committed);
    assert!(
        result.error.as_deref().is_some_and(|e| !e.is_empty()),
        "must carry a readable error"
    );
}

/// mock adapter + AutoApprover 走 quick.app_control launch，断言 committed。
/// notepad 在默认白名单内，launch 跳过审批（app_control.rs），无需审批交互。
#[cfg(all(windows, feature = "uia"))]
mod execute_skill_uia_tests {
    use trust_kernel::approval::approver::{AutoApprover, AutoDenier};
    use trust_kernel::error::Result;
    use trust_kernel::skills::dag_executor::set_thread_local_uia_adapter;
    use trust_kernel::uiautomation::{UiaAdapter, UiaElementHandle, UiaSelector};
    use voicepilot_ui::commands::{ExecuteSkillInput, execute_skill};
    use voicepilot_ui::state::AppState;

    struct MockAdapter;

    impl UiaAdapter for MockAdapter {
        fn launch_app(&self, _app_name: &str) -> Result<UiaElementHandle> {
            Ok(UiaElementHandle::mock())
        }
        fn find_window(&self, _title: &str) -> Result<Option<UiaElementHandle>> {
            Ok(Some(UiaElementHandle::mock()))
        }
        fn find_element(
            &self,
            _root: &UiaElementHandle,
            _selector: &UiaSelector,
        ) -> Result<Option<UiaElementHandle>> {
            Ok(Some(UiaElementHandle::mock()))
        }
        fn click(&self, _element: &UiaElementHandle) -> Result<()> {
            Ok(())
        }
        fn set_text(&self, _element: &UiaElementHandle, _text: &str) -> Result<()> {
            Ok(())
        }
        fn get_text(&self, _element: &UiaElementHandle) -> Result<String> {
            Ok(String::new())
        }
        fn screenshot(&self, _element: &UiaElementHandle) -> Result<Vec<u8>> {
            Ok(vec![])
        }
    }

    #[test]
    fn execute_skill_app_control_launch_with_mock_adapter_commits() {
        let state = AppState::new_in_memory().unwrap();
        set_thread_local_uia_adapter(Some(std::sync::Arc::new(MockAdapter)));
        let result = execute_skill(
            &state,
            &AutoApprover,
            &ExecuteSkillInput {
                task_id: "ui-mock-1".to_string(),
                step_id: "s-mock-1".to_string(),
                skill_id: "quick.app_control".to_string(),
                slots_json: serde_json::json!({"action": "launch", "app_name": "notepad"}),
            },
        )
        .unwrap();
        set_thread_local_uia_adapter(None);
        assert!(result.committed);
        assert!(result.error.is_none());
    }

    /// 白名单内 launch 即使 approver 全拒也提交（免审批直放行，app_control.rs）。
    #[test]
    fn execute_skill_whitelisted_launch_skips_denier() {
        let state = AppState::new_in_memory().unwrap();
        set_thread_local_uia_adapter(Some(std::sync::Arc::new(MockAdapter)));
        let result = execute_skill(
            &state,
            &AutoDenier,
            &ExecuteSkillInput {
                task_id: "ui-deny-skip-1".to_string(),
                step_id: "s-deny-skip-1".to_string(),
                skill_id: "quick.app_control".to_string(),
                slots_json: serde_json::json!({"action": "launch", "app_name": "notepad"}),
            },
        )
        .unwrap();
        set_thread_local_uia_adapter(None);
        assert!(result.committed);
        assert!(result.error.is_none());
    }

    /// 2026 免审批：白名单外 launch 不再弹审批 —— denier 也不拦截，直接提交。
    #[test]
    fn execute_skill_out_of_whitelist_launch_commits() {
        let state = AppState::new_in_memory().unwrap();
        set_thread_local_uia_adapter(Some(std::sync::Arc::new(MockAdapter)));
        let result = execute_skill(
            &state,
            &AutoDenier,
            &ExecuteSkillInput {
                task_id: "ui-deny-owl-1".to_string(),
                step_id: "s-deny-owl-1".to_string(),
                skill_id: "quick.app_control".to_string(),
                slots_json: serde_json::json!({"action": "launch", "app_name": "some-unlisted-app"}),
            },
        )
        .unwrap();
        set_thread_local_uia_adapter(None);
        assert!(
            result.committed,
            "免审批后应直接提交，错误: {:?}",
            result.error
        );
        assert!(result.error.is_none());
    }

    /// 2026 免审批：focus 不再弹审批 —— denier 也不拦截，直接提交。
    #[test]
    fn execute_skill_focus_on_whitelisted_app_commits() {
        let state = AppState::new_in_memory().unwrap();
        set_thread_local_uia_adapter(Some(std::sync::Arc::new(MockAdapter)));
        let result = execute_skill(
            &state,
            &AutoDenier,
            &ExecuteSkillInput {
                task_id: "ui-deny-focus-1".to_string(),
                step_id: "s-deny-focus-1".to_string(),
                skill_id: "quick.app_control".to_string(),
                slots_json: serde_json::json!({"action": "focus", "app_name": "notepad"}),
            },
        )
        .unwrap();
        set_thread_local_uia_adapter(None);
        assert!(
            result.committed,
            "免审批后应直接提交，错误: {:?}",
            result.error
        );
        assert!(result.error.is_none());
    }
}

/// P0: only UIA skills may trigger adapter construction. Non-UIA skills
/// (organize/files/research/…) must never fail for a missing/disabled
/// `mcp-windows` backend — `execute_skill_command` skips injection for them.
#[test]
fn needs_uia_adapter_gates_only_uia_skills() {
    use voicepilot_ui::commands::needs_uia_adapter;
    assert!(needs_uia_adapter("quick.app_control"));
    assert!(needs_uia_adapter("note.capture"));
    for other in [
        "files.organize",
        "task.repeat_verified",
        "task.explain",
        "task.compensate",
        "research.save_markdown",
        "form.prepare",
        "form.submit",
        "unknown.skill",
        "",
    ] {
        assert!(
            !needs_uia_adapter(other),
            "{other} must not need the adapter"
        );
    }
}

// ===== 追问卡（与审批语义分离：点选下标，超时回 default） =====

use voicepilot_ui::commands::submit_clarification;

#[test]
fn submit_clarification_delivers_index_to_waiter() {
    let state = AppState::new_in_memory().unwrap();
    let (id, rx) = state.approval_registry.create_clarify_request();
    let registry = state.approval_registry.clone();
    let handle = std::thread::spawn(move || {
        registry.wait_for_clarification(rx, std::time::Duration::from_secs(5), 0)
    });
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(submit_clarification(&state, &id, 2).unwrap());
    assert_eq!(handle.join().unwrap(), 2);
    // 一次性：重放返回 false。
    assert!(!submit_clarification(&state, &id, 1).unwrap());
}

#[test]
fn clarification_timeout_returns_default_not_deny() {
    let state = AppState::new_in_memory().unwrap();
    let (_id, rx) = state.approval_registry.create_clarify_request();
    // 无人投递：50ms 超时回 default_index。
    let got =
        state
            .approval_registry
            .wait_for_clarification(rx, std::time::Duration::from_millis(50), 1);
    assert_eq!(got, 1);
}
