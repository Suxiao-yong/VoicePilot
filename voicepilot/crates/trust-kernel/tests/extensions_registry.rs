//! RED contract tests for the future extension catalog.

use std::fs;
use std::path::PathBuf;

use trust_kernel::extensions::registry::ExtensionCatalog;
use trust_kernel::extensions::types::{ExecutionTarget, ExtensionDescriptor, ExtensionSource};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::mcp::repo::{McpServerRecord, McpServerRepo};
use trust_kernel::skills::manifest::{SkillExecutionSpec, SkillManifest, files_organize_manifest};
use uuid::Uuid;

fn manifest_for(id: &str) -> SkillManifest {
    let mut manifest = files_organize_manifest();
    manifest.id = id.to_string();
    manifest.title = format!("{id} fixture");
    manifest.intent_examples = vec![format!("run {id}")];
    manifest.keywords = vec!["read".to_string()];
    manifest.tools = vec![id.to_string()];
    manifest
}

fn descriptor(
    manifest: SkillManifest,
    source: ExtensionSource,
    target: Option<ExecutionTarget>,
    enabled: bool,
) -> ExtensionDescriptor {
    ExtensionDescriptor {
        version: manifest.version.clone(),
        manifest,
        source,
        target,
        enabled,
        manifest_hash: "sha256:contract-fixture".to_string(),
    }
}

fn user_skill_without_target() -> ExtensionDescriptor {
    let mut manifest = manifest_for("user.no-target");
    manifest.execution = None;
    descriptor(
        manifest,
        ExtensionSource::UserSkill {
            path: PathBuf::from("skills/no-target.md"),
        },
        None,
        true,
    )
}

fn catalog_with_disabled_mcp() -> ExtensionCatalog {
    let server_id = "example-readonly".to_string();
    let tool_name = "read".to_string();
    let mut manifest = manifest_for("example.read");
    manifest.tools = vec![tool_name.clone()];
    manifest.execution = Some(SkillExecutionSpec::McpTool {
        server_id: server_id.clone(),
        tool_name: tool_name.clone(),
    });

    let mut catalog = ExtensionCatalog::new();
    catalog
        .register(descriptor(
            manifest,
            ExtensionSource::McpPlugin {
                server_id: server_id.clone(),
            },
            Some(ExecutionTarget::McpTool {
                server_id,
                tool_name,
            }),
            false,
        ))
        .expect("disabled MCP fixture must register");
    catalog
}

fn mcp_tool_skill_md(skill_id: &str, server_id: &str, tool_name: &str) -> String {
    format!(
        r#"---
name: "{skill_id}"
description: "A complete MCP tool contract fixture. Runs {tool_name} on {server_id}."
metadata:
  voicepilot:
    execution:
      type: mcp_tool
      server_id: "{server_id}"
      tool_name: "{tool_name}"
---

# MCP tool contract fixture
"#,
        skill_id = skill_id,
        server_id = server_id,
        tool_name = tool_name,
    )
}

struct RestoreUserSkillsDirOnDrop {
    override_dir: PathBuf,
    backup_dir: PathBuf,
    armed: bool,
}

impl RestoreUserSkillsDirOnDrop {
    fn replace(override_dir: PathBuf) -> Self {
        let backup_dir =
            override_dir.with_file_name(format!("skills.rollback-backup-{}", Uuid::new_v4()));
        let mut guard = Self {
            override_dir,
            backup_dir,
            armed: false,
        };

        fs::rename(&guard.override_dir, &guard.backup_dir)
            .expect("rename temporary user Skills directory to a unique backup path");
        guard.armed = true;
        fs::write(&guard.override_dir, b"not a directory")
            .expect("replace temporary user Skills directory with a regular file");
        guard
    }
}

impl Drop for RestoreUserSkillsDirOnDrop {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let _ = fs::remove_file(&self.override_dir);
        let _ = fs::rename(&self.backup_dir, &self.override_dir);
    }
}

#[test]
fn candidates_preserve_registration_order_not_id_order() {
    let mut catalog = ExtensionCatalog::new();
    for id in ["z.last", "a.first", "m.middle"] {
        catalog
            .register(descriptor(
                manifest_for(id),
                ExtensionSource::Builtin,
                Some(ExecutionTarget::Builtin {
                    executor_id: id.to_string(),
                }),
                true,
            ))
            .expect("registration-order fixture must register");
    }

    let catalog_ids = catalog
        .candidates()
        .into_iter()
        .map(|descriptor| descriptor.manifest.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        catalog_ids,
        vec!["z.last", "a.first", "m.middle"],
        "candidates() must yield registration order (first-match-wins routing), not ID order"
    );

    let snapshot_ids = catalog
        .snapshot()
        .candidate_manifests()
        .into_iter()
        .map(|manifest| manifest.id)
        .collect::<Vec<_>>();
    assert_eq!(
        snapshot_ids,
        vec!["z.last", "a.first", "m.middle"],
        "snapshot candidate_manifests() must preserve registration order, not ID order"
    );
}

#[test]
fn user_skill_without_execution_target_is_not_a_candidate() {
    let mut catalog = ExtensionCatalog::new();
    catalog
        .register(user_skill_without_target())
        .expect("user skill fixture must register");

    assert!(catalog.enabled_manifests().is_empty());
}

#[test]
fn disabled_extension_is_rejected_even_when_called_directly() {
    let catalog = catalog_with_disabled_mcp();
    let error = catalog
        .resolve_execution_target("example.read")
        .unwrap_err();

    assert!(error.to_string().contains("disabled"));
}

#[test]
fn register_rejects_source_target_manifest_mismatch() {
    let mut manifest = manifest_for("user.mismatched-target");
    manifest.execution = None;
    let error = ExtensionCatalog::new()
        .register(descriptor(
            manifest,
            ExtensionSource::UserSkill {
                path: PathBuf::from("skills/mismatched-target.md"),
            },
            Some(ExecutionTarget::Builtin {
                executor_id: "files.organize".to_string(),
            }),
            true,
        ))
        .expect_err("a UserSkill cannot register with a Builtin execution target");

    let message = error.to_string().to_ascii_lowercase();
    assert!(
        message.contains("target") || message.contains("source") || message.contains("mismatch"),
        "source/target mismatch error should be informative: {error}"
    );
}

#[test]
fn builtin_registration_rejects_mcp_manifest_and_non_builtin_source() {
    let mut manifest = manifest_for("builtin.mcp-manifest");
    manifest.execution = Some(SkillExecutionSpec::McpTool {
        server_id: "fixture-server".to_string(),
        tool_name: "fixture-tool".to_string(),
    });
    let mut catalog = ExtensionCatalog::new();
    let error = catalog
        .register(descriptor(
            manifest,
            ExtensionSource::Builtin,
            Some(ExecutionTarget::Builtin {
                executor_id: "builtin.mcp-manifest".to_string(),
            }),
            true,
        ))
        .expect_err("builtin registration must reject MCP manifest execution");
    let message = error.to_string().to_ascii_lowercase();
    assert!(
        message.contains("builtin") && (message.contains("mcp") || message.contains("execution")),
        "builtin/MCP mismatch error should be informative: {error}"
    );

    let error = catalog
        .register_builtin(user_skill_without_target())
        .expect_err("register_builtin must reject non-Builtin sources");
    let message = error.to_string().to_ascii_lowercase();
    assert!(
        message.contains("builtin") && message.contains("source"),
        "register_builtin source error should be informative: {error}"
    );
}

#[test]
fn runtime_snapshot_reflects_skill_and_mcp_toggles() {
    let temp_root = tempfile::tempdir().expect("create temporary user Skills root");
    let skill_id = format!("contract.mcp-toggle.{}", Uuid::new_v4());
    let server_id = format!("contract-server-{}", Uuid::new_v4());
    let tool_name = "contract.tool";
    let skills_dir = temp_root.path().join("skills");
    fs::create_dir_all(&skills_dir).expect("create temporary user Skills directory");
    let kernel = TrustKernel::open_in_memory_with_user_skills_dir(skills_dir.clone())
        .expect("open in-memory kernel with temporary user Skills directory");
    let skill_path = skills_dir.join(format!("{skill_id}/SKILL.md"));
    std::fs::create_dir_all(skill_path.parent().expect("skill dir parent"))
        .expect("create skill dir");

    fs::write(
        &skill_path,
        mcp_tool_skill_md(&skill_id, &server_id, tool_name),
    )
    .expect("write MCP-backed user Skill");

    {
        let conn = kernel.conn();
        McpServerRepo::new()
            .create(
                &conn,
                &McpServerRecord {
                    server_id: server_id.clone(),
                    name: format!("Contract {server_id}"),
                    version: "1.0.0".to_string(),
                    transport: "stdio".to_string(),
                    enabled: true,
                    trusted: true,
                    protocol_version: Some("2025-11-25".to_string()),
                    allowed_origins: None,
                    allowed_paths: None,
                    command: None,
                    args: None,
                    env: None,
                },
            )
            .expect("create trusted enabled MCP server");
    }

    kernel.load_user_skills().expect("load user Skills");
    kernel
        .reload_extensions()
        .expect("reload extension catalog");
    assert!(
        kernel
            .extension_snapshot()
            .resolve_candidate(&skill_id)
            .is_some(),
        "trusted and enabled MCP-backed Skill must be a candidate"
    );

    kernel
        .toggle_skill(&skill_id, false)
        .expect("disable Skill");
    assert!(
        kernel
            .extension_snapshot()
            .resolve_candidate(&skill_id)
            .is_none(),
        "disabling a Skill must immediately remove its runtime candidate"
    );

    kernel
        .toggle_skill(&skill_id, true)
        .expect("re-enable Skill");
    assert!(
        kernel
            .extension_snapshot()
            .resolve_candidate(&skill_id)
            .is_some(),
        "re-enabling a Skill must immediately restore its runtime candidate"
    );

    kernel
        .toggle_mcp_server(&server_id, false)
        .expect("disable MCP server");
    assert!(
        kernel
            .extension_snapshot()
            .resolve_candidate(&skill_id)
            .is_none(),
        "disabling an MCP server must immediately remove dependent candidates"
    );

    kernel
        .toggle_mcp_server(&server_id, true)
        .expect("re-enable MCP server");
    assert!(
        kernel
            .extension_snapshot()
            .resolve_candidate(&skill_id)
            .is_some(),
        "re-enabling an MCP server must immediately restore dependent candidates"
    );
}

#[test]
fn toggle_skill_reload_failure_rolls_back_database_and_snapshot() {
    let temp_root = tempfile::tempdir().expect("create temporary user Skills root");
    let skill_id = format!("contract.mcp-rollback.{}", Uuid::new_v4());
    let server_id = format!("contract-server-{}", Uuid::new_v4());
    let tool_name = "contract.rollback-tool";
    let skills_dir = temp_root.path().join("skills");
    fs::create_dir_all(&skills_dir).expect("create temporary user Skills directory");
    let kernel = TrustKernel::open_in_memory_with_user_skills_dir(skills_dir.clone())
        .expect("open in-memory kernel with temporary user Skills directory");
    let skill_path = skills_dir.join(format!("{skill_id}/SKILL.md"));
    std::fs::create_dir_all(skill_path.parent().expect("skill dir parent"))
        .expect("create skill dir");

    fs::write(
        &skill_path,
        mcp_tool_skill_md(&skill_id, &server_id, tool_name),
    )
    .expect("write MCP-backed user Skill");

    {
        let conn = kernel.conn();
        McpServerRepo::new()
            .create(
                &conn,
                &McpServerRecord {
                    server_id: server_id.clone(),
                    name: format!("Contract {server_id}"),
                    version: "1.0.0".to_string(),
                    transport: "stdio".to_string(),
                    enabled: true,
                    trusted: true,
                    protocol_version: Some("2025-11-25".to_string()),
                    allowed_origins: None,
                    allowed_paths: None,
                    command: None,
                    args: None,
                    env: None,
                },
            )
            .expect("create trusted enabled MCP server");
    }

    kernel.load_user_skills().expect("load user Skills");
    let previous_snapshot = kernel.extension_snapshot();
    assert!(
        previous_snapshot.resolve_candidate(&skill_id).is_some(),
        "trusted and enabled MCP-backed Skill must initially be a candidate"
    );

    let _skills_dir_guard = RestoreUserSkillsDirOnDrop::replace(skills_dir);
    let error = kernel
        .toggle_skill(&skill_id, false)
        .expect_err("toggle must fail when extension reload cannot read the Skills directory");
    assert!(
        !error.to_string().is_empty(),
        "reload failure should preserve an informative error"
    );

    let persisted = {
        let conn = kernel.conn();
        kernel
            .skill_repo()
            .get(&conn, &skill_id)
            .expect("read persisted Skill row")
    }
    .expect("MCP-backed Skill row must remain present");
    assert!(
        persisted.enabled,
        "failed toggle reload must roll the Skill row back to enabled=true"
    );
    assert_eq!(
        kernel.extension_snapshot().id(),
        previous_snapshot.id(),
        "failed reload must preserve the previous in-memory snapshot"
    );
    assert!(
        kernel
            .extension_snapshot()
            .resolve_candidate(&skill_id)
            .is_some(),
        "previous in-memory snapshot must still resolve the MCP-backed Skill"
    );
}

#[test]
fn invalid_user_skill_target_does_not_block_builtin_catalog() {
    let temp_root = tempfile::tempdir().expect("create temporary user Skills root");
    let skill_id = format!("contract.invalid-mcp-target.{}", Uuid::new_v4());
    let skills_dir = temp_root.path().join("skills");
    fs::create_dir_all(&skills_dir).expect("create temporary user Skills directory");
    let kernel = TrustKernel::open_in_memory_with_user_skills_dir(skills_dir.clone())
        .expect("open in-memory kernel with temporary user Skills directory");
    let skill_path = skills_dir.join(format!("{skill_id}/SKILL.md"));
    std::fs::create_dir_all(skill_path.parent().expect("skill dir parent"))
        .expect("create skill dir");

    fs::write(
        &skill_path,
        mcp_tool_skill_md(&skill_id, "", "contract.invalid-tool"),
    )
    .expect("write invalid MCP-backed user Skill");

    let catalog = ExtensionCatalog::load(&kernel)
        .expect("an invalid user Skill target must not block builtin catalog loading");
    assert!(
        catalog.resolve_candidate("files.organize").is_some(),
        "builtin files.organize must resolve despite an invalid user Skill target"
    );
}

#[test]
fn load_registers_builtin_files_organize_as_enabled_candidate() {
    let temp_root = tempfile::tempdir().expect("create temporary user Skills root");
    let skills_dir = temp_root.path().join("skills");
    fs::create_dir_all(&skills_dir).expect("create temporary user Skills directory");
    let kernel = TrustKernel::open_in_memory_with_user_skills_dir(skills_dir)
        .expect("open in-memory kernel with temporary user Skills directory");
    let catalog = ExtensionCatalog::load(&kernel).expect("load extension catalog");

    assert!(
        catalog.resolve_candidate("files.organize").is_some(),
        "builtin files.organize must resolve as an enabled candidate"
    );
}
