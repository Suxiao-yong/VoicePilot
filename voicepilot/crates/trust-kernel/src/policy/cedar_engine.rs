//! Cedar authorization engine — V1.1 §4.2.
//!
//! Cedar produces binary allow/deny. The Rust Constraint Engine
//! (constraint_engine.rs) upgrades to ternary allow/confirm/deny based
//! on E×D risk classification.
//!
//! Cedar never modifies tool arguments — it only authorizes.

use crate::error::{KernelError, Result};
use crate::policy::types::{Action, Resource};
use cedar_policy::{
    Authorizer, Context, Decision, Entities, Entity, EntityId, EntityTypeName, EntityUid,
    PolicySet, Request, RestrictedExpression,
};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;

/// Compiled Cedar policy set. Cheap to clone (Arc inside).
#[derive(Clone)]
pub struct CedarEngine {
    policies: Arc<PolicySet>,
    authorizer: Authorizer,
}

impl CedarEngine {
    /// Parse Cedar source into a policy set.
    pub fn from_source(src: &str) -> Result<Self> {
        let policies: PolicySet = src
            .parse::<PolicySet>()
            .map_err(|e| KernelError::CedarParse(e.to_string()))?;
        Ok(Self {
            policies: Arc::new(policies),
            authorizer: Authorizer::new(),
        })
    }

    /// Return the SHA-256 hash of the policy source for bundle attribution.
    /// Computed on the original source string the engine was built from.
    pub fn bundle_hash(src: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(src.as_bytes());
        format!("sha256:{:x}", h.finalize())
    }

    /// Authorize a single (principal, action, resource) request.
    /// Returns true if Cedar permits (no forbid matches and at least one permit matches).
    pub fn is_allowed(&self, action: &Action, resource: &Resource) -> Result<bool> {
        let principal = make_uid("User", "voicepilot")?;
        let action_uid = make_uid("Action", &action.name)?;
        let resource_uid = make_uid("Resource", &resource.path)?;

        // Build the resource entity carrying `data_class`, `path`, and
        // `provenance` as attributes. Cedar's `when` clauses access these via
        // `resource.data_class` and `resource.path`; they must be entity
        // attributes, not Context entries.
        let mut attrs: HashMap<String, RestrictedExpression> = HashMap::new();
        attrs.insert(
            "data_class".to_string(),
            RestrictedExpression::new_string(resource.data_class.as_str().to_string()),
        );
        attrs.insert(
            "path".to_string(),
            RestrictedExpression::new_string(resource.path.clone()),
        );
        attrs.insert(
            "provenance".to_string(),
            RestrictedExpression::new_string(resource.provenance.clone()),
        );

        let resource_entity = Entity::new(resource_uid.clone(), attrs, HashSet::new())
            .map_err(|e| KernelError::CedarAuthz(e.to_string()))?;

        let entities = Entities::from_entities([resource_entity], None)
            .map_err(|e| KernelError::CedarAuthz(e.to_string()))?;

        let ctx = Context::empty();
        let request = Request::new(principal, action_uid, resource_uid, ctx, None)
            .map_err(|e| KernelError::CedarAuthz(e.to_string()))?;

        let response = self.authorizer.is_authorized(&request, &self.policies, &entities);
        Ok(response.decision() == Decision::Allow)
    }

    /// Return the list of policy IDs in the bundle (for audit).
    pub fn matched_policy_ids(&self) -> Vec<String> {
        self.policies
            .policies()
            .map(|p| p.id().to_string())
            .collect()
    }
}

fn make_uid(entity_type: &str, id: &str) -> Result<EntityUid> {
    let type_name = EntityTypeName::from_str(entity_type)
        .map_err(|e| KernelError::CedarAuthz(format!("invalid entity type {entity_type}: {e}")))?;
    let eid = EntityId::new(id);
    Ok(EntityUid::from_type_name_and_id(type_name, eid))
}
