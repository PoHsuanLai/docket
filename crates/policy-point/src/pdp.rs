//! The policy decision point over Cedar.

use crate::request::PolicyRequest;
use cedar_policy::{PolicySet, Schema, ValidationMode, Validator};
use docket_core::{PolicyId, Ruling};
use std::str::FromStr;

/// The schema, as shipped.
pub const SCHEMA: &str = include_str!("../../../policy/quire.cedarschema");
/// The default policies, as shipped.
pub const DEFAULT_POLICIES: &str = include_str!("../../../policy/default.cedar");

/// Why a schema or policy set could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    /// The schema does not parse.
    #[error("schema: {0}")]
    Schema(String),
    /// A policy does not parse.
    #[error("parse: {0}")]
    Parse(String),
    /// A policy does not validate against the schema (strict mode).
    #[error("invalid: {0}")]
    Invalid(String),
}

/// A loaded, validated policy set.
pub struct Pdp {
    policies: PolicySet,
    schema: Schema,
}

impl std::fmt::Debug for Pdp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pdp")
            .field("policies", &self.policy_ids().len())
            .finish_non_exhaustive()
    }
}

impl Pdp {
    /// Loads a schema and a policy set, validating the policies strictly against the schema.
    pub fn load(schema: &str, policies: &str) -> Result<Pdp, PolicyError> {
        let (schema, _warnings) =
            Schema::from_cedarschema_str(schema).map_err(|e| PolicyError::Schema(e.to_string()))?;
        let policies =
            PolicySet::from_str(policies).map_err(|e| PolicyError::Parse(e.to_string()))?;
        let result = Validator::new(schema.clone()).validate(&policies, ValidationMode::Strict);
        if result.validation_passed() {
            Ok(Pdp { policies, schema })
        } else {
            let faults: Vec<String> = result.validation_errors().map(|e| e.to_string()).collect();
            Err(PolicyError::Invalid(faults.join("; ")))
        }
    }

    /// Loads the shipped schema and default policies.
    pub fn standard() -> Result<Pdp, PolicyError> {
        Pdp::load(SCHEMA, DEFAULT_POLICIES)
    }

    /// The `@id` of every policy, in file order.
    pub fn policy_ids(&self) -> Vec<PolicyId> {
        self.policies
            .policies()
            .filter_map(|p| p.annotation("id").map(|id| PolicyId(id.to_owned())))
            .collect()
    }

    /// The schema the set was validated against.
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Rules on one request. Pure. `perform` not permitted is `Deny` (with the forbid ids, or
    /// none when no policy permitted it); else `perform_unasked` not permitted is `Ask` with
    /// the reasons the forbids name; else `perform_unjudged` not permitted is `AllowJudged`;
    /// else `AllowFinal`. Any other operation is one query: permitted is `AllowFinal`, else
    /// `Deny`. A request Cedar cannot evaluate is a `Deny` with no ids: failure never permits.
    pub fn decide(&self, request: &PolicyRequest) -> Ruling {
        crate::eval::decide(&self.policies, &self.schema, request)
    }
}
