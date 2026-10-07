use chrono::Utc;
use trust_kernel::toolresult::{EvidenceStrength, ToolResult, ToolStatus};

#[test]
fn toolresult_serializes_with_all_v2_fields() {
    let now = Utc::now();
    let r = ToolResult {
        status: ToolStatus::Succeeded,
        data: serde_json::json!({"moved": 3}),
        evidence_strength: EvidenceStrength::Strong,
        compensation_ref: Some("comp-1".to_string()),
        compensation_level: trust_kernel::compensation::types::CompensationLevel::Strong,
        preconditions_hash: Some("sha256:abc".to_string()),
        idempotency_key: "idem-1".to_string(),
        egress_performed: false,
        data_classification: trust_kernel::policy::types::DLevel::D1,
        error_code: None,
        retryable: false,
        safe_to_retry: true,
        started_at: now,
        finished_at: now,
    };
    let s = serde_json::to_string(&r).unwrap();
    assert!(s.contains("\"status\":\"succeeded\""));
    assert!(s.contains("\"evidence_strength\":\"strong\""));
    assert!(s.contains("\"compensation_level\":\"strong\""));
    assert!(s.contains("\"idempotency_key\":\"idem-1\""));
    assert!(s.contains("\"data_classification\":\"D1\""));
}

#[test]
fn evidence_strength_serializes_as_lowercase() {
    assert_eq!(
        serde_json::to_string(&EvidenceStrength::Strong).unwrap(),
        "\"strong\""
    );
    assert_eq!(
        serde_json::to_string(&EvidenceStrength::Medium).unwrap(),
        "\"medium\""
    );
    assert_eq!(
        serde_json::to_string(&EvidenceStrength::Weak).unwrap(),
        "\"weak\""
    );
}

#[test]
fn tool_status_round_trips() {
    for s in [
        ToolStatus::Succeeded,
        ToolStatus::Failed,
        ToolStatus::Cancelled,
        ToolStatus::Partial,
    ] {
        let json = serde_json::to_string(&s).unwrap();
        let back: ToolStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }
}
