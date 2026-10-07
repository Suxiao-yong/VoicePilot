//! 外部 Skill 导入：复制 SKILL.md 本体 + 上架 + 默认关闭。

use std::fs;
use trust_kernel::kernel::TrustKernel;

const GOOD_SKILL: &str =
    "---\nname: ext-imp\ndescription: External import test skill.\n---\n\n# Body\n\nText.\n";

fn kernel_with_empty_skills_dir() -> (TrustKernel, tempfile::TempDir, std::path::PathBuf) {
    let temp_root = tempfile::tempdir().expect("temp root");
    let skills_dir = temp_root.path().join("skills");
    fs::create_dir_all(&skills_dir).expect("skills dir");
    let kernel =
        TrustKernel::open_in_memory_with_user_skills_dir(skills_dir.clone()).expect("kernel");
    (kernel, temp_root, skills_dir)
}

fn write_external_skill(root: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    let dir = root.join(name);
    fs::create_dir_all(&dir).expect("ext dir");
    fs::write(dir.join("SKILL.md"), body).expect("write SKILL.md");
    // 顺带放一个脚本文件：导入必须只复制 SKILL.md，不跟随。
    fs::write(dir.join("evil.py"), "print(1)").expect("write decoy");
    dir
}

#[test]
fn import_external_skill_copies_manifest_only_and_starts_disabled() {
    let (kernel, temp_root, skills_dir) = kernel_with_empty_skills_dir();
    let src = write_external_skill(temp_root.path(), "src-skill", GOOD_SKILL);

    let manifest = kernel.import_external_skill(&src).expect("import ok");
    assert_eq!(manifest.id, "ext-imp");

    // 只复制了 SKILL.md，脚本不跟随。
    assert!(skills_dir.join("ext-imp").join("SKILL.md").is_file());
    assert!(!skills_dir.join("ext-imp").join("evil.py").exists());

    // 第三方内容默认关闭（与自有目录新 Skill 默认启用不同）。
    let conn = kernel.conn();
    let rec = trust_kernel::skills::repo::SkillRepo::new()
        .get(&conn, "ext-imp")
        .expect("db read")
        .expect("row must exist");
    assert!(!rec.enabled, "external import must start disabled");
    // 行内容必须等于返回的 manifest（出生即 disabled，无“先启用再关闭”窗口）。
    let row_manifest: serde_json::Value =
        serde_json::from_str(&rec.manifest_json).expect("row manifest parses");
    assert_eq!(
        row_manifest.get("id").and_then(|v| v.as_str()),
        Some(manifest.id.as_str()),
        "row content must match the validated manifest"
    );
    // 落盘文件必须等于已校验字节（不回读源盘，无读-拷竞态）。
    let on_disk =
        std::fs::read_to_string(skills_dir.join("ext-imp").join("SKILL.md")).expect("read dest");
    let (reparsed, _) =
        trust_kernel::skills::user_loader::parse_skill_md(&on_disk).expect("dest parses");
    assert_eq!(reparsed.id, manifest.id);
}

#[test]
fn import_external_skill_rejects_duplicate_broken_and_relative() {
    let (kernel, temp_root, _skills_dir) = kernel_with_empty_skills_dir();
    let src = write_external_skill(temp_root.path(), "src-skill", GOOD_SKILL);
    kernel.import_external_skill(&src).expect("first import ok");

    // 同 id 再次导入：显式报错，不覆盖。
    let src2 = write_external_skill(temp_root.path(), "other-dir", GOOD_SKILL);
    let err = kernel.import_external_skill(&src2).unwrap_err();
    assert!(
        format!("{err:?}").contains("already exists"),
        "unexpected: {err:?}"
    );

    // 无 SKILL.md 的目录。
    let empty = temp_root.path().join("empty-dir");
    fs::create_dir_all(&empty).unwrap();
    assert!(kernel.import_external_skill(&empty).is_err());

    // 非法 frontmatter。
    let bad = write_external_skill(temp_root.path(), "bad", "no frontmatter here");
    assert!(kernel.import_external_skill(&bad).is_err());

    // 相对路径拒绝。
    assert!(
        kernel
            .import_external_skill(std::path::Path::new("relative/path"))
            .is_err()
    );
}
