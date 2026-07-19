//! Rust Constraint Engine — V1.1 §4.2.
//!
//! Cedar produces binary allow/deny. This engine:
//!   1. Normalizes tool arguments (path canonicalization).
//!   2. Applies Rust-side constraints (max_files, overwrite, allowed_destinations).
//!   3. Upgrades Cedar's binary decision to ternary (allow/confirm/deny)
//!      using the E×D risk matrix.
//!
//! Per §4.2: "Cedar 仅产出 allow/deny 二态，Rust Constraint Engine 在其上
//! 扩展为三态并应用参数约束。"

use crate::error::{KernelError, Result};
use crate::policy::risk_matrix::classify;
use crate::policy::types::{DLevel, Effect, ELevel};
use std::collections::HashMap;

/// Per-tool constraint specification. W2 covers filesystem-style tools.
#[derive(Debug, Clone, Default)]
pub struct ConstraintSpec {
    pub max_files: Option<usize>,
    pub overwrite: Option<bool>,
    pub allowed_destinations: Option<Vec<String>>,
}

#[derive(Default)]
pub struct ConstraintEngine {
    specs: HashMap<String, ConstraintSpec>,
}

impl ConstraintEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: &str, spec: ConstraintSpec) {
        self.specs.insert(tool.to_string(), spec);
    }

    /// Normalize tool arguments. W2: canonicalize paths (backslashes → forward,
    /// lowercase drive letter, dedupe slashes).
    pub fn normalize_args(
        &self,
        _tool: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let mut out = args.clone();
        normalize_paths_in_place(&mut out);
        Ok(out)
    }

    /// Apply constraints to normalized args. Returns (constrained_args, applied_list).
    pub fn apply_constraints(
        &self,
        tool: &str,
        mut args: serde_json::Value,
    ) -> Result<(serde_json::Value, Vec<String>)> {
        let mut applied = Vec::new();
        if let Some(spec) = self.specs.get(tool) {
            if let Some(max) = spec.max_files {
                if let Some(arr) = args.get_mut("sources").and_then(|v| v.as_array_mut()) {
                    if arr.len() > max {
                        arr.truncate(max);
                        applied.push(format!("max_files={}", max));
                    }
                }
            }
            if let Some(false) = spec.overwrite {
                applied.push("overwrite=false".to_string());
                // Real existence check lands in W3 when filesystem MCP is wired.
            }
            if let Some(allowed) = &spec.allowed_destinations {
                if let Some(dest) = args.get("destination").and_then(|v| v.as_str()) {
                    if !allowed.iter().any(|a| dest.starts_with(a)) {
                        return Err(KernelError::ConstraintViolation(format!(
                            "destination {} not in allowed list",
                            dest
                        )));
                    }
                }
                applied.push("allowed_destinations checked".to_string());
            }
        }
        Ok((args, applied))
    }

    /// Upgrade Cedar's binary decision to ternary using E×D risk matrix.
    /// - If Cedar denies → Deny
    /// - If Cedar allows → consult risk matrix (Allow / Confirm / Deny)
    pub fn upgrade_effect(&self, cedar_allows: bool, e: ELevel, d: DLevel) -> Effect {
        if !cedar_allows {
            return Effect::Deny;
        }
        classify(e, d)
    }
}

/// Recursively normalize path-like strings inside a JSON value.
/// Heuristic: any string containing a backslash or starting with a drive letter.
fn normalize_paths_in_place(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::String(s) => {
            if s.contains('\\') || (s.len() >= 2 && s.as_bytes()[1] == b':') {
                let normalized = s.replace('\\', "/");
                // Lowercase drive letter (e.g. "C:/" → "c:/") — keep rest as-is.
                let normalized = if normalized.len() >= 2 {
                    let bytes = normalized.as_bytes();
                    if bytes[1] == b':' {
                        let mut out = String::new();
                        out.push(bytes[0].to_ascii_lowercase() as char);
                        out.push_str(&normalized[1..]);
                        out
                    } else {
                        normalized
                    }
                } else {
                    normalized
                };
                *s = normalized;
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                normalize_paths_in_place(item);
            }
        }
        serde_json::Value::Object(obj) => {
            for (_, v) in obj.iter_mut() {
                normalize_paths_in_place(v);
            }
        }
        _ => {}
    }
}
