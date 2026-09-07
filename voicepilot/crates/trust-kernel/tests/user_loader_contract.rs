use std::fs;

use trust_kernel::kernel::TrustKernel;
use trust_kernel::skills::repo::SkillRecord;
use trust_kernel::skills::user_loader::{scan_user_skill_files, UserSkillFile};

// Standard Agent Skills frontmatter (name/description) fixture.
fn valid_skill_md(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Contract test skill for file scanning.\n---\n\n# Body text\n\nThis is the description body.\n",
        name = name,
    )
}

// Write a standard skill at `{root}/{name}/SKILL.md`.
fn write_standard_skill(root: &std::path::Path, name: &str) -> std::path::PathBuf {
    let dir = root.join(name);
    fs::create_dir_all(&dir).expect("create skill directory");
    let skill_md = dir.join("SKILL.md");
    fs::write(&skill_md, valid_skill_md(name)).expect("write skill");
    skill_md
}

#[test]
fn scan_user_skill_files_preserves_path_and_manifest_id() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let skill_id = format!("contract-scan-{}", uuid::Uuid::new_v4());
    let skill_path = write_standard_skill(temp_dir.path(), &skill_id);

    let files: Vec<UserSkillFile> = scan_user_skill_files(temp_dir.path());
    assert_eq!(files.len(), 1, "expected one valid user Skill file");
    assert_eq!(files[0].path, skill_path);
    assert_eq!(files[0].manifest.id, skill_id);

    fs::remove_dir_all(temp_dir.path()).expect("remove temporary directory");
}

#[test]
fn scan_user_skill_files_skips_oversized_file() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let skill_path = write_standard_skill(temp_dir.path(), "contract-oversize");
    // Exceed MAX_SKILL_FILE_BYTES (1 MiB) so the loader's resource-exhaustion
    // guard skips the file before YAML parsing.
    let mut content = fs::read_to_string(&skill_path).expect("read skill");
    content.push_str(&"x".repeat(1024 * 1024 + 1));
    fs::write(&skill_path, content).expect("write oversized skill");

    let files: Vec<UserSkillFile> = scan_user_skill_files(temp_dir.path());
    assert!(
        files.is_empty(),
        "oversized skill file must be skipped (resource-exhaustion guard)"
    );
    fs::remove_dir_all(temp_dir.path()).expect("remove temporary directory");
}

#[test]
fn load_user_skills_preserves_existing_skill_state() {
    let temp_root = tempfile::tempdir().expect("create temporary user Skills root");
    let skills_dir = temp_root.path().join("skills");
    fs::create_dir_all(&skills_dir).expect("create temporary user Skills directory");
    let skill_id = format!("contract-reload-{}", uuid::Uuid::new_v4());
    let skill_path = write_standard_skill(&skills_dir, &skill_id);
    assert!(skill_path.is_file());

    let kernel = TrustKernel::open_in_memory_with_user_skills_dir(skills_dir)
        .expect("open in-memory kernel with temporary user Skills directory");
    let existing = SkillRecord {
        skill_id: skill_id.clone(),
        version: 1,
        manifest_json: serde_json::json!({"id": skill_id}).to_string(),
        enabled: false,
        success_count: 7,
        avg_latency_ms: 123.5,
    };
    {
        let conn = kernel.conn();
        kernel
            .skill_repo()
            .upsert(&conn, &existing)
            .expect("upsert existing Skill row");
        kernel
            .skill_repo()
            .toggle(&conn, &skill_id, existing.enabled)
            .expect("set existing Skill enabled state");
        for _ in 0..existing.success_count {
            kernel
                .skill_repo()
                .incr_success(&conn, &skill_id, existing.avg_latency_ms)
                .expect("record existing Skill success state");
        }
    }

    kernel.load_user_skills().expect("reload user skills");
    let reloaded = {
        let conn = kernel.conn();
        kernel
            .skill_repo()
            .get(&conn, &skill_id)
            .expect("load Skill row")
            .expect("reloaded Skill row exists")
    };

    assert!(!reloaded.enabled, "reload must preserve disabled state");
    assert_eq!(reloaded.success_count, 7);
    assert!((reloaded.avg_latency_ms - 123.5).abs() < f64::EPSILON);
}
