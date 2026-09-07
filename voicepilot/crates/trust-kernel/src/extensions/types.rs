use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{KernelError, Result};
use crate::skills::manifest::SkillManifest;

/// Where an extension manifest came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtensionSource {
    Builtin,
    UserSkill { path: PathBuf },
    McpPlugin { server_id: String },
}

/// The execution backend selected for an extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionTarget {
    Builtin {
        executor_id: String,
    },
    McpTool {
        server_id: String,
        tool_name: String,
    },
}

/// One manifest plus its source, executable target, and catalog state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionDescriptor {
    pub manifest: SkillManifest,
    pub source: ExtensionSource,
    pub target: Option<ExecutionTarget>,
    pub enabled: bool,
    pub version: String,
    pub manifest_hash: String,
}

/// Immutable clone data captured for one task.
#[derive(Debug, Clone)]
pub struct ExtensionSnapshot {
    pub(crate) snapshot_id: String,
    pub(crate) entries: BTreeMap<String, ExtensionDescriptor>,
    /// Registration order of manifest ids; `candidates()` yields this order
    /// (first-match-wins routing), not BTreeMap ID order.
    pub(crate) order: Vec<String>,
}

impl ExtensionSnapshot {
    pub fn snapshot_id(&self) -> &str {
        &self.snapshot_id
    }

    /// Alias for callers that use the shorter snapshot identifier name.
    pub fn id(&self) -> &str {
        self.snapshot_id()
    }

    /// All entries, including disabled and display-only extensions.
    pub fn entries(&self) -> &BTreeMap<String, ExtensionDescriptor> {
        &self.entries
    }

    /// Planner candidates: enabled entries with an execution target, in
    /// registration order (first-match-wins routing).
    pub fn candidates(&self) -> Vec<&ExtensionDescriptor> {
        self.order
            .iter()
            .filter_map(|id| self.entries.get(id))
            .filter(|descriptor| descriptor.enabled && descriptor.target.is_some())
            .collect()
    }

    pub fn candidate_manifests(&self) -> Vec<SkillManifest> {
        self.candidates()
            .into_iter()
            .map(|descriptor| descriptor.manifest.clone())
            .collect()
    }

    /// Enabled planner candidates, excluding display-only entries.
    pub fn enabled_manifests(&self) -> Vec<SkillManifest> {
        self.candidate_manifests()
    }

    /// Resolve only an enabled planner candidate.
    pub fn resolve_candidate(&self, id: &str) -> Option<&ExtensionDescriptor> {
        self.entries
            .get(id)
            .filter(|descriptor| descriptor.enabled && descriptor.target.is_some())
    }

    /// Resolve a target for direct execution, preserving useful rejection errors
    /// for disabled, unknown, and display-only entries.
    pub fn resolve_execution_target(&self, id: &str) -> Result<ExecutionTarget> {
        resolve_execution_target(&self.entries, id)
    }
}

pub(crate) fn resolve_execution_target(
    entries: &BTreeMap<String, ExtensionDescriptor>,
    id: &str,
) -> Result<ExecutionTarget> {
    let descriptor = entries
        .get(id)
        .ok_or_else(|| KernelError::Skill(format!("extension '{id}' was not found")))?;
    if !descriptor.enabled {
        return Err(KernelError::Skill(format!("extension '{id}' is disabled")));
    }
    descriptor
        .target
        .clone()
        .ok_or_else(|| KernelError::Skill(format!("extension '{id}' has no execution target")))
}
