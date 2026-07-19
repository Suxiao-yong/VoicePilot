//! Action Gateway facade — V1.1 §4.2, §4.4, §6.2.
//!
//! Orchestrates the full policy pipeline per §4.4:
//!   1. Normalize resource (path canonicalization, provenance).
//!   2. Hard-deny rules (D3, shell, taint elevation).
//!   3. Cedar authorization (binary allow/deny).
//!   4. Rust Constraint Engine (args normalization + constraints).
//!   5. E×D risk classification (ternary allow/confirm/deny).
//!   6. Egress check (if data flows to remote destination).
//!   7. Return Decision with effect + reasons + matched_policies +
//!      normalized_args + constraints_applied + approval_scope + policy_bundle_hash.

use crate::error::Result;
use crate::policy::cedar_engine::CedarEngine;
use crate::policy::constraint_engine::{ConstraintEngine, ConstraintSpec};
use crate::policy::egress::check_egress;
use crate::policy::types::{Action, Decision, EgressDest, Resource, Effect};
use std::sync::Arc;

pub struct ActionGateway {
    cedar: CedarEngine,
    constraints: ConstraintEngine,
    bundle_hash: String,
    bundle_src: Arc<String>,
}

impl ActionGateway {
    /// Build a gateway from a Cedar source string.
    pub fn new(cedar_src: &str) -> Result<Self> {
        Ok(Self {
            cedar: CedarEngine::from_source(cedar_src)?,
            constraints: ConstraintEngine::new(),
            bundle_hash: CedarEngine::bundle_hash(cedar_src),
            bundle_src: Arc::new(cedar_src.to_string()),
        })
    }

    /// Register a per-tool Rust constraint.
    pub fn register_constraint(&mut self, tool: &str, spec: ConstraintSpec) {
        self.constraints.register(tool, spec);
    }

    /// The SHA-256 hash of the active Cedar policy bundle.
    pub fn bundle_hash(&self) -> &str {
        &self.bundle_hash
    }

    /// Full policy decision pipeline.
    ///
    /// `egress_dest`: Some(_) if the tool sends data to a remote destination.
    /// `args`: tool arguments (will be normalized + constrained).
    pub fn decide(
        &self,
        tool: &str,
        e_level: crate::policy::types::ELevel,
        resource: &Resource,
        egress_dest: Option<EgressDest>,
        args: Option<serde_json::Value>,
    ) -> Result<Decision> {
        let action = Action { name: tool.to_string(), e_level };
        let mut reasons: Vec<String> = Vec::new();

        // Step 2: hard-deny rules (D3, shell_exec, taint elevation).
        if resource.data_class == crate::policy::types::DLevel::D3 {
            return Ok(Decision::deny(
                self.bundle_hash.clone(),
                "D3 red line: credentials never enter model context",
            ));
        }
        if tool == "shell_exec" {
            return Ok(Decision::deny(
                self.bundle_hash.clone(),
                "shell_exec always denied",
            ));
        }
        // Taint elevation: web_page provenance cannot become tool argument.
        if resource.provenance == "web_page" && egress_dest == Some(EgressDest::ToolArgument) {
            return Ok(Decision::deny(
                self.bundle_hash.clone(),
                "taint elevation: web_page cannot become tool argument",
            ));
        }

        // Step 3: Cedar authorization.
        let cedar_allows = self.cedar.is_allowed(&action, resource)?;
        if !cedar_allows {
            return Ok(Decision::deny(
                self.bundle_hash.clone(),
                "Cedar denied",
            ));
        }

        // Step 4: normalize args + apply constraints.
        let (normalized_args, constraints_applied) = if let Some(args) = args {
            let normalized = self.constraints.normalize_args(tool, &args)?;
            let (constrained, applied) = self.constraints.apply_constraints(tool, normalized)?;
            (constrained, applied)
        } else {
            (serde_json::Value::Null, vec![])
        };

        // Step 5: E×D risk classification.
        let mut effect = self.constraints.upgrade_effect(cedar_allows, e_level, resource.data_class);
        if matches!(effect, Effect::Deny) {
            reasons.push(format!("E{:?}×D{:?} = deny", e_level, resource.data_class));
        } else if matches!(effect, Effect::Confirm) {
            reasons.push(format!("E{:?}×D{:?} = confirm", e_level, resource.data_class));
        }

        // Step 6: egress check.
        // For remote destinations (RemoteLlm/RemoteMcp/ToolArgument), egress
        // overrides E×D — §4.3 is the specific rule for egress flows.
        // For LocalFile, egress returns Allow as a placeholder ("E×D covers it"
        // per egress.rs), so more_restrictive preserves the E×D decision.
        if let Some(dest) = egress_dest {
            let egress_effect = check_egress(resource.data_class, dest);
            effect = if dest == EgressDest::LocalFile {
                more_restrictive(effect, egress_effect)
            } else {
                egress_effect
            };
            if matches!(egress_effect, Effect::Deny) {
                reasons.push(format!("egress to {:?} denied for D{:?}", dest, resource.data_class));
            } else if matches!(egress_effect, Effect::Confirm) {
                reasons.push(format!("egress to {:?} requires confirm for D{:?}", dest, resource.data_class));
            }
        }

        // Step 7: assemble Decision.
        let matched_policies = self.cedar.matched_policy_ids(&action, resource)?;
        let approval_scope = if matches!(effect, Effect::Confirm) { "single" } else { "single" };

        Ok(Decision {
            effect,
            matched_policies,
            normalized_args,
            constraints_applied,
            approval_scope: approval_scope.to_string(),
            policy_bundle_hash: self.bundle_hash.clone(),
            reasons,
        })
    }

    /// Reference to the original Cedar source (for audit / persistence).
    pub fn cedar_source(&self) -> &str {
        &self.bundle_src
    }
}

fn more_restrictive(a: Effect, b: Effect) -> Effect {
    use Effect::*;
    match (a, b) {
        (Deny, _) | (_, Deny) => Deny,
        (Confirm, _) | (_, Confirm) => Confirm,
        (Allow, Allow) => Allow,
    }
}
