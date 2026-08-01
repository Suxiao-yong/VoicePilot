#![cfg(feature = "tauri")]

//! audit_commands 单元测试 —— DTO 转换 + command 逻辑。

use trust_kernel::audit::AuditEvent;
use voicepilot_ui::audit_commands::AuditEventDto;

#[test]
fn audit_event_dto_converts_from_kernel_event() {
    let event = AuditEvent {
        log_id: "log-1".to_string(),
        task_id: "task-1".to_string(),
        step_id: Some("step-1".to_string()),
        event_type: "task_created".to_string(),
        details: serde_json::json!({"k": "v"}),
        timestamp: chrono::Utc::now(),
        prev_hash: None,
        hash: "abc".to_string(),
    };
    let dto = AuditEventDto::from(event);
    assert_eq!(dto.log_id, "log-1");
    assert_eq!(dto.task_id, "task-1");
    assert_eq!(dto.step_id.as_deref(), Some("step-1"));
    assert_eq!(dto.event_type, "task_created");
    assert_eq!(dto.hash, "abc");
    assert!(dto.timestamp.contains("T")); // RFC3339
}

#[test]
fn audit_event_dto_handles_none_step_id() {
    let event = AuditEvent {
        log_id: "log-2".to_string(),
        task_id: "task-2".to_string(),
        step_id: None,
        event_type: "task_created".to_string(),
        details: serde_json::Value::Null,
        timestamp: chrono::Utc::now(),
        prev_hash: Some("prev".to_string()),
        hash: "def".to_string(),
    };
    let dto = AuditEventDto::from(event);
    assert!(dto.step_id.is_none());
    assert_eq!(dto.prev_hash.as_deref(), Some("prev"));
}
