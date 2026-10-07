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
    Authorizer, Context, Decision, Entities, Entity, EntityId, EntityTypeName, EntityUid, PolicyId,
    PolicySet, Request, RestrictedExpression,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Compiled Cedar policy set. Cheap to clone (Arc inside).
#[derive(Clone, Debug)]
pub struct CedarEngine {
    policies: Arc<PolicySet>,
    authorizer: Authorizer,
}

impl CedarEngine {
    /// Parse Cedar source into a policy set.
    ///
    /// In Cedar 4.x, `@id("...")` is an annotation, not the policy ID.
    /// We apply the annotation as the actual ID so `matched_policy_ids()`
    /// returns human-readable IDs for audit (V1.1 §8.1).
    pub fn from_source(src: &str) -> Result<Self> {
        let parsed: PolicySet = src
            .parse::<PolicySet>()
            .map_err(|e| KernelError::CedarParse(e.to_string()))?;
        let mut policies = PolicySet::new();
        for p in parsed.policies() {
            let new_p = match p.annotation("id") {
                Some(id_str) => {
                    let new_id: PolicyId = id_str.parse().map_err(|e| {
                        KernelError::CedarParse(format!("invalid @id {id_str:?}: {e}"))
                    })?;
                    p.new_id(new_id)
                }
                None => p.clone(),
            };
            policies
                .add(new_p)
                .map_err(|e| KernelError::CedarParse(e.to_string()))?;
        }
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
    ///
    /// **W2 note:** Diagnostics (matched policy IDs, errors) are discarded here.
    /// Use `matched_policy_ids()` to get the matched policy IDs for audit.
    /// A future task may expose full diagnostics via a richer return type.
    pub fn is_allowed(&self, action: &Action, resource: &Resource) -> Result<bool> {
        let (request, entities) = self.build_request(action, resource)?;
        let response = self
            .authorizer
            .is_authorized(&request, &self.policies, &entities);
        Ok(response.decision() == Decision::Allow)
    }

    /// Return the policy IDs that matched (either permitted or forbade) the given request.
    /// Useful for audit logging — V1.1 §8.1 `matched_policies` field.
    pub fn matched_policy_ids(&self, action: &Action, resource: &Resource) -> Result<Vec<String>> {
        let (request, entities) = self.build_request(action, resource)?;
        let response = self
            .authorizer
            .is_authorized(&request, &self.policies, &entities);
        Ok(response
            .diagnostics()
            .reason()
            .map(|p| p.to_string())
            .collect())
    }

    /// Build the Cedar `Request` and `Entities` for a single (principal, action, resource) tuple.
    /// Shared by `is_allowed` and `matched_policy_ids` to avoid duplication.
    fn build_request(&self, action: &Action, resource: &Resource) -> Result<(Request, Entities)> {
        let principal = make_uid("User", "voicepilot");
        let action_uid = make_uid("Action", &action.name);
        let resource_uid = make_uid("Resource", &resource.path);

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

        // Resources in V1 have no parent entities (no hierarchy).
        let parents: HashSet<EntityUid> = HashSet::new();
        let resource_entity = Entity::new(resource_uid.clone(), attrs, parents)
            .map_err(|e| KernelError::CedarAuthz(e.to_string()))?;

        let entities = Entities::from_entities([resource_entity], None)
            .map_err(|e| KernelError::CedarAuthz(e.to_string()))?;

        let ctx = Context::empty();
        // No schema for W2 PoC — schemaless mode (V1.1 §4.2).
        let request = Request::new(principal, action_uid, resource_uid, ctx, None)
            .map_err(|e| KernelError::CedarAuthz(e.to_string()))?;
        Ok((request, entities))
    }
}

fn make_uid(entity_type: &str, id: &str) -> EntityUid {
    let type_name: EntityTypeName = entity_type
        .parse()
        .expect("hardcoded entity type strings are valid Cedar entity types");
    EntityUid::from_type_name_and_id(type_name, EntityId::new(id))
}
