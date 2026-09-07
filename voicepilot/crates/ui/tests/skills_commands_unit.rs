#![cfg(feature = "tauri")]

use trust_kernel::kernel::TrustKernel;
use trust_kernel::skills::repo::SkillRecord;
use voicepilot_ui::skills_commands::{
    import_external_skill, scan_external_skills, scan_external_skills_with_roots, SkillDto,
};
use voicepilot_ui::state::AppState;

fn state_with_empty_skills_dir() -> (AppState, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let skills_dir = tmp.path().join("skills");
    std::fs::create_dir_all(&skills_dir).expect("skills dir");
    let kernel = TrustKernel::open_in_memory_with_user_skills_dir(skills_dir).expect("kernel");
    (AppState::new(kernel), tmp)
}

fn write_external_skill(root: &std::path::Path) -> std::path::PathBuf {
    let dir = root.join("ext-skill");
    std::fs::create_dir_all(&dir).expect("ext dir");
    std::fs::write(
        dir.join("SKILL.md"),
        "---\nname: ext-skill\ndescription: External skill for UI test.\n---\n\nBody.\n",
    )
    .expect("write");
    dir
}

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

#[test]
fn scan_external_skills_runs_without_error() {
    // 只读扫描：真实机器上可能命中也可能为空，断言仅为“不炸”。
    let (state, _tmp) = state_with_empty_skills_dir();
    let hits = scan_external_skills(&state).expect("scan ok");
    for h in &hits {
        assert!(!h.id.is_empty());
        assert!(!h.source.is_empty());
    }
}

#[test]
fn import_external_skill_lands_disabled() {
    let (state, tmp) = state_with_empty_skills_dir();
    let src = write_external_skill(tmp.path());
    let dto = import_external_skill(&state, src.to_str().unwrap()).expect("import ok");
    assert_eq!(dto.skill_id, "ext-skill");
    // 默认关闭：用户须在 Skills Manager 手动启用。此处直接查 DB 行，
    // 而非经 list_user_skills（后者 DTO 无 enabled 字段，查不出 posture）。
    let conn = state.kernel.conn();
    let rec = trust_kernel::skills::repo::SkillRepo::new()
        .get(&conn, "ext-skill")
        .expect("db read")
        .expect("row must exist");
    assert!(!rec.enabled, "imported external skill must start disabled");
    let skills = voicepilot_ui::skills_commands::list_user_skills(&state).expect("list");
    assert!(skills.iter().any(|s| s.skill_id == "ext-skill"));
}

#[test]
fn scan_external_skills_with_roots_maps_every_field() {
    // TempDir 固件逐字段断言映射：交换任两字段即变红。含一条 MCP 绑定
    // skill，覆盖 executable/exec_server/exec_tool 的 true 分支。
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("ext-roots");
    let plain = root.join("plain-skill");
    std::fs::create_dir_all(&plain).expect("plain dir");
    std::fs::write(
        plain.join("SKILL.md"),
        "---\nname: plain-skill\ndescription: Plain external skill.\n---\n\nBody.\n",
    )
    .expect("write");
    let bound = root.join("bound-skill");
    std::fs::create_dir_all(&bound).expect("bound dir");
    std::fs::write(
        bound.join("SKILL.md"),
        "---\nname: bound-skill\ndescription: Bound external skill.\nmetadata:\n  voicepilot:\n    execution:\n      type: mcp_tool\n      server_id: srv-1\n      tool_name: do_thing\n---\n\nBody.\n",
    )
    .expect("write");
    let hits = scan_external_skills_with_roots(&[(root.clone(), "fixture")]);
    assert_eq!(hits.len(), 2);
    let plain_hit = hits.iter().find(|h| h.id == "plain-skill").unwrap();
    assert_eq!(plain_hit.source, "fixture");
    assert_eq!(plain_hit.title, "plain-skill");
    assert_eq!(plain_hit.description, "Plain external skill.");
    assert!(!plain_hit.executable);
    assert_eq!(plain_hit.exec_server, None);
    assert_eq!(plain_hit.exec_tool, None);
    assert_eq!(plain_hit.dir, plain.to_string_lossy().into_owned());
    let bound_hit = hits.iter().find(|h| h.id == "bound-skill").unwrap();
    assert!(bound_hit.executable);
    assert_eq!(bound_hit.exec_server.as_deref(), Some("srv-1"));
    assert_eq!(bound_hit.exec_tool.as_deref(), Some("do_thing"));
}
