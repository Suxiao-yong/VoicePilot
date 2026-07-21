#![cfg(feature = "tauri")]

//! W6b-2 端到端冒烟测试 —— V1.1.2 §8.3 Settings/Audit/Trust Center/Skills Manager。
//!
//! 验证四个子系统的后端 repo CRUD 与 TrustKernel facade 编排:
//! - Settings(ConfigRepo KV round-trip)
//! - Skills Manager(SkillRecord upsert/list/toggle/get)
//! - Trust Center(McpServerRepo create + toggle_mcp_server)
//! - Audit Viewer(list_recent / list_for_task)

use trust_kernel::kernel::TrustKernel;
use trust_kernel::mcp::repo::{McpServerRecord, McpServerRepo};
use trust_kernel::repo::step_repo::StepRecord;
use trust_kernel::skills::repo::SkillRecord;

#[test]
fn settings_kv_round_trip() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    kernel.config_repo().set(&conn, "theme", "dark").unwrap();
    kernel.config_repo().set(&conn, "whisper_model", "ggml-base.bin").unwrap();
    let v1 = kernel.config_repo().get(&conn, "theme").unwrap();
    assert_eq!(v1.as_deref(), Some("dark"));
    let v2 = kernel.config_repo().get(&conn, "whisper_model").unwrap();
    assert_eq!(v2.as_deref(), Some("ggml-base.bin"));
    kernel.config_repo().set(&conn, "theme", "light").unwrap();
    let v3 = kernel.config_repo().get(&conn, "theme").unwrap();
    assert_eq!(v3.as_deref(), Some("light"));
}

#[test]
fn skills_manager_crud() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    let rec = SkillRecord {
        skill_id: "test.echo".to_string(),
        version: 1,  // i64(001_init.sql skills.version INTEGER)
        manifest_json: r#"{"id":"test.echo","risk":"E1D1"}"#.to_string(),
        enabled: true,
        success_count: 0,
        avg_latency_ms: 0.0,
    };
    kernel.skill_repo().upsert(&conn, &rec).unwrap();
    let listed = kernel.skill_repo().list(&conn).unwrap();
    assert!(listed.iter().any(|s| s.skill_id == "test.echo"));
    kernel.skill_repo().toggle(&conn, "test.echo", false).unwrap();
    let got = kernel.skill_repo().get(&conn, "test.echo").unwrap().unwrap();
    assert!(!got.enabled);
}

#[test]
fn trust_center_toggle_mcp_server() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let record = McpServerRecord {
        server_id: "test.svc".to_string(),
        name: "Test".to_string(),
        version: "1.0".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: false,
        protocol_version: Some("2025-11-25".to_string()),
        allowed_origins: None,
        allowed_paths: Some("[]".to_string()),
    };
    // conn 锁必须先释放再调用 kernel.toggle_mcp_server —— 后者内部
    // 会再次 self.conn() 获取同一 Mutex(Rust Mutex 不可重入,否则死锁)。
    {
        let conn = kernel.conn();
        McpServerRepo::new().create(&conn, &record).unwrap();
    }
    kernel.toggle_mcp_server("test.svc", false).unwrap();
    {
        let conn = kernel.conn();
        let loaded = McpServerRepo::new().get(&conn, "test.svc").unwrap().unwrap();
        assert!(!loaded.enabled);
    }
    kernel.toggle_mcp_server("test.svc", true).unwrap();
    {
        let conn = kernel.conn();
        let loaded2 = McpServerRepo::new().get(&conn, "test.svc").unwrap().unwrap();
        assert!(loaded2.enabled);
    }
}

#[test]
fn audit_viewer_lists_recent_events() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let task_id = "w6b2-audit-task";
    let step_id = "w6b2-audit-step";
    kernel.create_task(task_id, "test user goal").unwrap();
    let step = StepRecord::new(step_id, task_id, 1);
    kernel.create_step(&step).unwrap();
    kernel
        .audit_append_external(task_id, Some(step_id), "STEP_STARTED", serde_json::json!({"k":"v"}))
        .unwrap();
    let events = kernel.list_audit_recent(10).unwrap();
    assert!(!events.is_empty());
    let for_task = kernel.list_audit_for_task(task_id).unwrap();
    assert!(!for_task.is_empty());
}
