use std::collections::{BTreeMap, HashSet};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::mcp::repo::{McpServerRecord, McpServerRepo};
use crate::skills::manifest::{
    SkillExecutionSpec, SkillManifest, files_organize_manifest, form_prepare_manifest,
    form_submit_manifest, research_save_manifest, task_compensate_manifest, task_explain_manifest,
    task_repeat_verified_manifest,
};
// Daisy 移植 Skill 的 manifest 与各域模块同处（manifest.rs 只留旧 9 个）。
use crate::skills::clip_ops::{
    clip_press_keys_manifest, clip_read_manifest, clip_save_image_manifest,
    clip_selected_files_manifest, clip_selected_manifest, clip_type_text_manifest,
    clip_write_manifest,
};
use crate::skills::doc_office::doc_office_manifest;
use crate::skills::fs_ops::{
    fs_create_manifest, fs_delete_manifest, fs_list_manifest, fs_read_manifest, fs_write_manifest,
};
use crate::skills::media_ops::{
    clip_chorus_manifest, doc_convert_manifest, media_convert_manifest, media_download_manifest,
    media_trim_manifest,
};
use crate::skills::pim::{
    mail_compose_manifest, pim_calendar_list_manifest, pim_calendar_manifest, pim_note_manifest,
    pim_reminder_manifest, pim_search_manifest,
};
use crate::skills::repo::SkillRepo;
use crate::skills::shell_run::shell_run_manifest;
use crate::skills::sys_ops::{
    sys_alarm_set_manifest, sys_datetime_manifest, sys_diagnose_app_manifest,
    sys_frontmost_manifest, sys_lock_screen_manifest, sys_maps_search_manifest,
    sys_open_url_manifest, sys_playback_manifest, sys_quit_all_manifest,
    sys_quit_browsers_manifest, sys_timer_set_manifest, sys_volume_manifest,
};
use crate::skills::user_loader::scan_user_skill_files;
use crate::skills::web_ops::{
    web_fetch_file_manifest, web_scrape_manifest, web_search_manifest, web_sports_manifest,
    web_wallpapers_manifest, web_weather_manifest,
};

use super::types::{
    ExecutionTarget, ExtensionDescriptor, ExtensionSnapshot, ExtensionSource,
    resolve_execution_target,
};

#[derive(Debug, Clone)]
pub struct ExtensionCatalog {
    snapshot_id: String,
    entries: BTreeMap<String, ExtensionDescriptor>,
    /// Registration order of manifest ids; `candidates()` yields this order
    /// (first-match-wins routing), not BTreeMap ID order.
    order: Vec<String>,
}

impl Default for ExtensionCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtensionCatalog {
    pub fn new() -> Self {
        Self {
            snapshot_id: Uuid::new_v4().to_string(),
            entries: BTreeMap::new(),
            order: Vec::new(),
        }
    }

    /// Build a catalog from the existing builtin manifests, MCP configuration,
    /// and user Skill files. This only reads configuration; it never starts an
    /// MCP process or calls tools/list.
    pub fn load(kernel: &TrustKernel) -> Result<Self> {
        let mut catalog = Self::new();

        for manifest in builtin_manifests() {
            let target = ExecutionTarget::Builtin {
                executor_id: manifest.id.clone(),
            };
            catalog.register_builtin(ExtensionDescriptor {
                version: manifest.version.clone(),
                manifest,
                source: ExtensionSource::Builtin,
                target: Some(target),
                enabled: true,
                manifest_hash: String::new(),
            })?;
        }

        // MCP rows are the only source of server trust/enabled state. Reading
        // and structurally validating them is intentionally process-free.
        let executable_servers = {
            let conn = kernel.conn();
            McpServerRepo::new()
                .list(&conn)?
                .into_iter()
                .filter(|server| {
                    server.enabled && server.trusted && is_structurally_valid_mcp_server(server)
                })
                .map(|server| server.server_id)
                .collect::<HashSet<_>>()
        };

        let dir = kernel.user_skills_dir()?;
        for user_skill in scan_user_skill_files(&dir) {
            let skill_enabled = {
                let conn = kernel.conn();
                SkillRepo::new()
                    .get(&conn, &user_skill.manifest.id)?
                    .map(|record| record.enabled)
                    .unwrap_or(true)
            };

            let (target, target_available) = match user_skill.manifest.execution.clone() {
                Some(SkillExecutionSpec::McpTool {
                    server_id,
                    tool_name,
                }) => {
                    let available = !server_id.trim().is_empty()
                        && !tool_name.trim().is_empty()
                        && executable_servers.contains(&server_id);
                    (
                        Some(ExecutionTarget::McpTool {
                            server_id,
                            tool_name,
                        }),
                        available,
                    )
                }
                None => (None, false),
            };

            let enabled = skill_enabled && (target_available || target.is_none());
            let skill_id = user_skill.manifest.id.clone();
            let skill_path = user_skill.path.clone();
            if let Err(error) = catalog.register(ExtensionDescriptor {
                version: user_skill.manifest.version.clone(),
                manifest: user_skill.manifest,
                source: ExtensionSource::UserSkill {
                    path: user_skill.path,
                },
                target,
                enabled,
                manifest_hash: String::new(),
            }) {
                tracing::warn!(
                    skill_load_error = ?skill_path,
                    skill_id = %skill_id,
                    error = ?error,
                    "skipping invalid user Skill descriptor"
                );
            }
        }

        Ok(catalog)
    }

    pub fn snapshot(&self) -> ExtensionSnapshot {
        ExtensionSnapshot {
            snapshot_id: self.snapshot_id.clone(),
            entries: self.entries.clone(),
            order: self.order.clone(),
        }
    }

    pub fn snapshot_id(&self) -> &str {
        &self.snapshot_id
    }

    /// All entries, including disabled and display-only extensions.
    pub fn entries(&self) -> &BTreeMap<String, ExtensionDescriptor> {
        &self.entries
    }

    /// Register an entry. Later registrations replace earlier entries with the
    /// same manifest ID, matching SkillRouter's user-overrides-builtin rule;
    /// replacement keeps the entry's original registration position.
    pub fn register(&mut self, mut descriptor: ExtensionDescriptor) -> Result<()> {
        validate_descriptor(&descriptor)?;
        descriptor.manifest_hash = canonical_manifest_hash(&descriptor.manifest)?;
        let id = descriptor.manifest.id.clone();
        if !self.entries.contains_key(&id) {
            self.order.push(id.clone());
        }
        self.entries.insert(id, descriptor);
        Ok(())
    }

    pub fn register_builtin(&mut self, descriptor: ExtensionDescriptor) -> Result<()> {
        if !matches!(&descriptor.source, ExtensionSource::Builtin) {
            return Err(KernelError::Skill(format!(
                "builtin registration requires Builtin source for extension '{}'",
                descriptor.manifest.id
            )));
        }
        self.register(descriptor)
    }

    /// Resolve only an enabled planner candidate.
    pub fn resolve_candidate(&self, id: &str) -> Option<&ExtensionDescriptor> {
        self.entries
            .get(id)
            .filter(|descriptor| descriptor.enabled && descriptor.target.is_some())
    }

    pub fn candidates(&self) -> Vec<&ExtensionDescriptor> {
        self.order
            .iter()
            .filter_map(|id| self.entries.get(id))
            .filter(|descriptor| descriptor.enabled && descriptor.target.is_some())
            .collect()
    }

    pub fn enabled_manifests(&self) -> Vec<SkillManifest> {
        self.candidates()
            .into_iter()
            .map(|descriptor| descriptor.manifest.clone())
            .collect()
    }

    /// Resolve a target for direct execution. Disabled and display-only entries
    /// remain in the catalog so callers receive an explicit rejection.
    pub fn resolve_execution_target(&self, id: &str) -> Result<ExecutionTarget> {
        resolve_execution_target(&self.entries, id)
    }
}

fn validate_descriptor(descriptor: &ExtensionDescriptor) -> Result<()> {
    if descriptor.manifest.id.trim().is_empty() {
        return Err(KernelError::Skill(
            "extension manifest id must not be blank".to_string(),
        ));
    }
    if descriptor.version != descriptor.manifest.version {
        return Err(KernelError::Skill(format!(
            "extension '{}' descriptor version does not match manifest version",
            descriptor.manifest.id
        )));
    }

    if let ExtensionSource::McpPlugin { server_id } = &descriptor.source {
        validate_mcp_id("MCP plugin server id", server_id)?;
    }
    if let Some(ExecutionTarget::McpTool {
        server_id,
        tool_name,
    }) = descriptor.target.as_ref()
    {
        validate_mcp_id("MCP target server id", server_id)?;
        validate_mcp_id("MCP target tool name", tool_name)?;
    }
    if let Some(SkillExecutionSpec::McpTool {
        server_id,
        tool_name,
    }) = descriptor.manifest.execution.as_ref()
    {
        validate_mcp_id("manifest MCP server id", server_id)?;
        validate_mcp_id("manifest MCP tool name", tool_name)?;
    }

    match (&descriptor.source, descriptor.target.as_ref()) {
        (ExtensionSource::Builtin, Some(ExecutionTarget::Builtin { executor_id }))
            if executor_id == &descriptor.manifest.id =>
        {
            if descriptor.manifest.execution.is_some() {
                return Err(KernelError::Skill(format!(
                    "builtin extension '{}' must not declare manifest execution",
                    descriptor.manifest.id
                )));
            }
            Ok(())
        }
        (ExtensionSource::Builtin, _) => Err(KernelError::Skill(format!(
            "builtin extension '{}' must use a matching Builtin execution target",
            descriptor.manifest.id
        ))),
        (ExtensionSource::UserSkill { .. }, None) if descriptor.manifest.execution.is_none() => {
            Ok(())
        }
        (
            ExtensionSource::UserSkill { .. },
            Some(ExecutionTarget::McpTool {
                server_id,
                tool_name,
            }),
        ) => match descriptor.manifest.execution.as_ref() {
            Some(SkillExecutionSpec::McpTool {
                server_id: manifest_server_id,
                tool_name: manifest_tool_name,
            }) if server_id == manifest_server_id && tool_name == manifest_tool_name => Ok(()),
            _ => Err(KernelError::Skill(format!(
                "user Skill '{}' MCP target does not match manifest execution",
                descriptor.manifest.id
            ))),
        },
        (ExtensionSource::UserSkill { .. }, Some(ExecutionTarget::Builtin { .. })) => {
            Err(KernelError::Skill(format!(
                "user Skill '{}' cannot use a Builtin execution target",
                descriptor.manifest.id
            )))
        }
        (ExtensionSource::UserSkill { .. }, None) => Err(KernelError::Skill(format!(
            "user Skill '{}' requires a matching MCP target for manifest execution",
            descriptor.manifest.id
        ))),
        (
            ExtensionSource::McpPlugin { server_id },
            Some(ExecutionTarget::McpTool {
                server_id: target_server_id,
                tool_name: target_tool_name,
            }),
        ) => match descriptor.manifest.execution.as_ref() {
            Some(SkillExecutionSpec::McpTool {
                server_id: manifest_server_id,
                tool_name: manifest_tool_name,
            }) if server_id == target_server_id
                && target_server_id == manifest_server_id
                && target_tool_name == manifest_tool_name =>
            {
                Ok(())
            }
            _ => Err(KernelError::Skill(format!(
                "MCP plugin '{}' target does not match its manifest execution",
                descriptor.manifest.id
            ))),
        },
        (ExtensionSource::McpPlugin { .. }, _) => Err(KernelError::Skill(format!(
            "MCP plugin '{}' must use a matching MCP tool target",
            descriptor.manifest.id
        ))),
    }
}

fn validate_mcp_id(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(KernelError::Skill(format!("{label} must not be blank")));
    }
    Ok(())
}

fn builtin_manifests() -> Vec<SkillManifest> {
    let mut manifests = Vec::new();
    manifests.extend([
        files_organize_manifest(),
        task_repeat_verified_manifest(),
        // task_compensate 必须在 task_explain 之前,否则 task_explain 的 keyword '上一步'
        // 会先匹配 '撤销上一步'(first-match-wins,同旧 router_bridge)。
        task_compensate_manifest(),
        task_explain_manifest(),
        research_save_manifest(),
        form_prepare_manifest(),
        form_submit_manifest(),
        // Daisy 移植 30 Skill（旧 9 个顺序不动，first-match-wins 保护既有
        // 行为；新 Skill 内部 specific-before-generic，如闹钟先于时间）。
        sys_alarm_set_manifest(),
        sys_timer_set_manifest(),
        sys_volume_manifest(),
        sys_playback_manifest(),
        sys_lock_screen_manifest(),
        sys_quit_browsers_manifest(),
        sys_datetime_manifest(),
        sys_frontmost_manifest(),
        sys_quit_all_manifest(),
        sys_diagnose_app_manifest(),
        sys_open_url_manifest(),
        sys_maps_search_manifest(),
        clip_selected_manifest(),
        clip_save_image_manifest(),
        clip_selected_files_manifest(),
        clip_type_text_manifest(),
        clip_press_keys_manifest(),
        clip_read_manifest(),
        clip_write_manifest(),
        fs_read_manifest(),
        fs_list_manifest(),
        fs_write_manifest(),
        fs_create_manifest(),
        fs_delete_manifest(),
        web_search_manifest(),
        web_scrape_manifest(),
        // 通用资源下载（搜索 → 挑直链 → yt-dlp generic 落盘）。
        web_fetch_file_manifest(),
        web_wallpapers_manifest(),
        web_weather_manifest(),
        web_sports_manifest(),
        shell_run_manifest(),
        media_download_manifest(),
        media_trim_manifest(),
        media_convert_manifest(),
        clip_chorus_manifest(),
        doc_convert_manifest(),
        doc_office_manifest(),
        mail_compose_manifest(),
        pim_note_manifest(),
        pim_search_manifest(),
        pim_reminder_manifest(),
        pim_calendar_manifest(),
        pim_calendar_list_manifest(),
        // Phase B：跨会话长期记忆的用户否决面（可看可删）。
        crate::memory::memory_view_manifest(),
        crate::memory::memory_forget_manifest(),
        // Phase C：进程内后台作业（创建/列表/停用）。
        crate::scheduler::task_schedule_manifest(),
        crate::scheduler::task_jobs_manifest(),
        crate::scheduler::task_unschedule_manifest(),
    ]);

    #[cfg(all(windows, feature = "uia"))]
    {
        use crate::skills::manifest::{app_control_manifest, note_capture_manifest};
        manifests.push(app_control_manifest());
        manifests.push(note_capture_manifest());
    }

    manifests
}

fn is_structurally_valid_mcp_server(server: &McpServerRecord) -> bool {
    if [
        server.server_id.as_str(),
        server.name.as_str(),
        server.version.as_str(),
        server.transport.as_str(),
    ]
    .iter()
    .any(|value| value.trim().is_empty())
    {
        return false;
    }
    if server
        .protocol_version
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
        || server
            .command
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
    {
        return false;
    }

    valid_json_string_array(server.allowed_origins.as_deref())
        && valid_json_string_array(server.allowed_paths.as_deref())
        && valid_json_string_array(server.args.as_deref())
        && valid_json_string_map(server.env.as_deref())
}

fn valid_json_string_array(value: Option<&str>) -> bool {
    value
        .map(|raw| serde_json::from_str::<Vec<String>>(raw).is_ok())
        .unwrap_or(true)
}

fn valid_json_string_map(value: Option<&str>) -> bool {
    value
        .map(|raw| serde_json::from_str::<std::collections::HashMap<String, String>>(raw).is_ok())
        .unwrap_or(true)
}

fn canonical_manifest_hash(manifest: &SkillManifest) -> Result<String> {
    let value = serde_json::to_value(manifest)?;
    let canonical = canonicalize_json(value);
    let bytes = serde_json::to_vec(&canonical)?;
    let digest = Sha256::digest(bytes);
    Ok(format!("sha256:{digest:x}"))
}

/// Recursively sort object keys so the same JSON value always canonicalizes
/// to the same bytes (stable hashing regardless of insertion order).
/// pub(crate): reused by the dispatcher's MCP tool schema hash.
pub(crate) fn canonicalize_json(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => {
            let mut fields = object.into_iter().collect::<Vec<_>>();
            fields.sort_by(|left, right| left.0.cmp(&right.0));
            let mut canonical = serde_json::Map::new();
            for (key, value) in fields {
                canonical.insert(key, canonicalize_json(value));
            }
            serde_json::Value::Object(canonical)
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(canonicalize_json).collect())
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::manifest::files_organize_manifest;

    #[test]
    fn canonical_hash_is_stable_for_hashmap_insertion_order() {
        let mut first = files_organize_manifest();
        let mut second = first.clone();
        first.inputs = first.inputs.into_iter().collect();
        let mut second_entries = second.inputs.into_iter().collect::<Vec<_>>();
        second_entries.reverse();
        second.inputs = second_entries.into_iter().collect();

        assert_eq!(
            canonical_manifest_hash(&first).unwrap(),
            canonical_manifest_hash(&second).unwrap()
        );
    }
}
