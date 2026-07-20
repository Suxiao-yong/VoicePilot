//! SkillManifest — V1.1 §5.3 + Appendix C.
//!
//! Each Skill fixes: usable tools, max risk, param schema, max steps,
//! approval mode, verifier, compensation, egress. Built-in Skills are
//! Rust struct literals; user-saved Skills load from YAML in W7.

use crate::compensation::types::{CompensationLevel, ConflictPolicy};
use crate::policy::types::{DLevel, ELevel};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub id: String,
    pub version: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub intent_examples: Vec<String>,
    pub inputs: HashMap<String, SkillInput>,
    pub risk_ceiling: ELevel,
    pub data_class_ceiling: DLevel,
    pub egress: EgressKind,
    pub max_steps: u32,
    pub tools: Vec<String>,
    pub approval: ApprovalConfig,
    pub compensation: CompensationConfig,
    pub verifier: VerifierConfig,
    pub failure_policy: FailurePolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInput {
    pub input_type: SkillInputType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub allowed_roots: Vec<String>,
    #[serde(default)]
    pub allowed_values: Vec<String>,
    #[serde(default)]
    pub max_length: Option<u32>,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillInputType {
    Directory,
    File,
    FileFilter,
    Text,
    Number,
    Enum,
    Url,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressKind {
    LocalOnly,
    WebToLocal,
    LocalToWebDraft,
    LocalToWebSubmit,
}

impl EgressKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EgressKind::LocalOnly => "local_only",
            EgressKind::WebToLocal => "web_to_local",
            EgressKind::LocalToWebDraft => "local_to_web_draft",
            EgressKind::LocalToWebSubmit => "local_to_web_submit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    None,
    PerStep,
    BatchOnce,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalConfig {
    pub mode: ApprovalMode,
    /// "prepare" | "commit" | "both"
    pub required_for: String,
    pub show_effect_manifest: bool,
    pub max_approval_scope: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CompensationConfig {
    pub level: CompensationLevel,
    pub ttl_seconds: u32,
    pub conflict_policy: ConflictPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierConfig {
    /// "strong" | "medium" | "weak"
    pub strategy: String,
    pub recheck_after_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailurePolicy {
    pub max_retries: u32,
    pub allow_replan: bool,
    /// "stop" | "ask_user" | "compensate"
    pub on_fail: String,
}

/// The built-in files.organize Skill manifest — V1.1 §5.2 + §5.3 example.
pub fn files_organize_manifest() -> SkillManifest {
    let mut inputs = std::collections::HashMap::new();
    inputs.insert(
        "source".to_string(),
        SkillInput {
            input_type: SkillInputType::Directory,
            required: true,
            allowed_roots: vec![
                "Downloads".to_string(),
                "Desktop".to_string(),
                "Workspace".to_string(),
            ],
            allowed_values: vec![],
            max_length: None,
            default: None,
        },
    );
    inputs.insert(
        "filter".to_string(),
        SkillInput {
            input_type: SkillInputType::FileFilter,
            required: true,
            allowed_roots: vec![],
            allowed_values: vec![],
            max_length: None,
            default: None,
        },
    );
    inputs.insert(
        "destination".to_string(),
        SkillInput {
            input_type: SkillInputType::Directory,
            required: true,
            allowed_roots: vec![
                "Workspace".to_string(),
                "Documents".to_string(),
                "Desktop".to_string(),
            ],
            allowed_values: vec![],
            max_length: None,
            default: None,
        },
    );

    SkillManifest {
        id: "files.organize".to_string(),
        version: "1.0.0".to_string(),
        title: "整理文件".to_string(),
        description: "搜索文件 → 生成变更清单 → 一次性批次批准 → 移动并验证 → 生成 strong Compensation".to_string(),
        intent_examples: vec![
            "把下载目录里的 PDF 移到论文文件夹".to_string(),
            "整理今天下载的文档".to_string(),
            "把下载的 PDF 整理到项目文件夹".to_string(),
        ],
        inputs,
        risk_ceiling: ELevel::E2,
        data_class_ceiling: DLevel::D2,
        egress: EgressKind::LocalOnly,
        max_steps: 4,
        tools: vec![
            "filesystem.search_files".to_string(),
            "filesystem.prepare_move".to_string(),
            "filesystem.commit_move".to_string(),
            "filesystem.verify_move".to_string(),
        ],
        approval: ApprovalConfig {
            mode: ApprovalMode::BatchOnce,
            required_for: "commit".to_string(),
            show_effect_manifest: true,
            max_approval_scope: 3,
        },
        compensation: CompensationConfig {
            level: CompensationLevel::Strong,
            ttl_seconds: 3600,
            conflict_policy: ConflictPolicy::RequireConfirmation,
        },
        verifier: VerifierConfig {
            strategy: "strong".to_string(),
            recheck_after_seconds: 0,
        },
        failure_policy: FailurePolicy {
            max_retries: 1,
            allow_replan: false,
            on_fail: "stop".to_string(),
        },
    }
}
