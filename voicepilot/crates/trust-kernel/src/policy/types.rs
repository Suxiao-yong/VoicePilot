//! Core policy types — V1.1 §4.1 (E×D matrix), §4.2 (Cedar + Constraint), §6.3 (ToolResult V2).

use serde::{Deserialize, Serialize};

/// Operation risk level — V1.1 §4.1.
/// E0: pure read; E1: local reversible; E2: local important; E3: irreversible/egress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ELevel {
    E0,
    E1,
    E2,
    E3,
}

/// Data sensitivity — V1.1 §4.1.
/// D0: public; D1: personal; D2: private docs; D3: credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum DLevel {
    D0,
    D1,
    D2,
    D3,
}

impl DLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            DLevel::D0 => "D0",
            DLevel::D1 => "D1",
            DLevel::D2 => "D2",
            DLevel::D3 => "D3",
        }
    }
}

/// Final ternary effect after all policy layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Allow,
    Confirm,
    Deny,
}

/// Resource being acted upon — V1.1 §4.2 Cedar resource entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub path: String,
    pub data_class: DLevel,
    /// Provenance tag — V1.1 §7.3 taint tracking. One of:
    /// user_direct | web_page | external_doc | tool_output
    pub provenance: String,
}

/// Action being requested — maps to Cedar Action entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    /// Action name — e.g. "read_file", "move_files", "send_to_remote_llm".
    pub name: String,
    pub e_level: ELevel,
}

/// Egress destination — V1.1 §4.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressDest {
    LocalFile,
    RemoteLlm,
    RemoteMcp,
    ToolArgument,
}

impl EgressDest {
    /// Returns the snake_case string for this destination.
    /// NOTE: must match the variant names produced by `#[serde(rename_all = "snake_case")]`
    /// above. Used for Cedar entity UID construction in Task 4.
    pub fn as_str(self) -> &'static str {
        match self {
            EgressDest::LocalFile => "local_file",
            EgressDest::RemoteLlm => "remote_llm",
            EgressDest::RemoteMcp => "remote_mcp",
            EgressDest::ToolArgument => "tool_argument",
        }
    }
}

/// Final decision returned by the Action Gateway.
/// Mirrors V1.1 §4.2 output: effect + reasons + matched_policies +
/// normalized_args + constraints_applied + approval_scope + policy_bundle_hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub effect: Effect,
    pub matched_policies: Vec<String>,
    pub normalized_args: serde_json::Value,
    pub constraints_applied: Vec<String>,
    pub approval_scope: String, // "single" | "batch"
    pub policy_bundle_hash: String,
    pub reasons: Vec<String>,
}

impl Decision {
    pub fn allow(hash: impl Into<String>) -> Self {
        Self {
            effect: Effect::Allow,
            matched_policies: vec![],
            normalized_args: serde_json::Value::Null,
            constraints_applied: vec![],
            approval_scope: "single".to_string(),
            policy_bundle_hash: hash.into(),
            reasons: vec![],
        }
    }

    pub fn deny(hash: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            effect: Effect::Deny,
            matched_policies: vec![],
            normalized_args: serde_json::Value::Null,
            constraints_applied: vec![],
            approval_scope: "single".to_string(),
            policy_bundle_hash: hash.into(),
            reasons: vec![reason.into()],
        }
    }
}
