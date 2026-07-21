#![cfg(feature = "tauri")]

use trust_kernel::skills::repo::SkillRecord;
use voicepilot_ui::skills_commands::SkillDto;

#[test]
fn skill_dto_extracts_risk_label_from_manifest() {
    let rec = SkillRecord {
        skill_id: "files.organize".to_string(),
        version: 1,
        manifest_json: r#"{"id":"files.organize","risk":"E2D2"}"#.to_string(),
        enabled: true,
        success_count: 5,
        avg_latency_ms: 1234.5,
    };
    let dto = SkillDto::from(rec);
    assert_eq!(dto.skill_id, "files.organize");
    assert_eq!(dto.risk_label, "E2D2");
    assert_eq!(dto.success_count, 5);
    assert_eq!(dto.version, "1");
}

#[test]
fn skill_dto_defaults_risk_label_when_manifest_invalid() {
    let rec = SkillRecord {
        skill_id: "x".to_string(),
        version: 1,
        manifest_json: "not json".to_string(),
        enabled: false,
        success_count: 0,
        avg_latency_ms: 0.0,
    };
    let dto = SkillDto::from(rec);
    assert_eq!(dto.risk_label, "unknown");
}
