//! The policy decision point over Cedar.

use crate::fault::Ruled;
use crate::layers::PolicySource;
use crate::request::PolicyRequest;
use cedar_policy::{PolicySet, Schema, ValidationMode, Validator};
use docket_core::{PolicyId, Ruling};
use std::str::FromStr;

/// The schema, as shipped.
pub const SCHEMA: &str = include_str!("../../../policy/quire.cedarschema");
/// The core policies, as shipped: the safety invariants of every preset.
pub const CORE_POLICIES: &str = include_str!("../../../policy/core.cedar");
/// The standard preset, as shipped: today's grid. Also what stands in for an installed preset
/// that is missing or does not load.
pub const STANDARD_PRESET: &str = include_str!("../../../policy/presets/standard/grid.cedar");

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
        Pdp::checked(policies, schema)
    }

    /// Validates `policies` strictly against `schema` and holds them.
    pub(crate) fn checked(policies: PolicySet, schema: Schema) -> Result<Pdp, PolicyError> {
        let result = Validator::new(schema.clone()).validate(&policies, ValidationMode::Strict);
        if result.validation_passed() {
            Ok(Pdp { policies, schema })
        } else {
            let faults: Vec<String> = result.validation_errors().map(|e| e.to_string()).collect();
            Err(PolicyError::Invalid(faults.join("; ")))
        }
    }

    /// Loads the shipped schema with the built-in core and standard preset: no files are read,
    /// so tests and the fake router get the same grid whatever is installed.
    pub fn standard() -> Result<Pdp, PolicyError> {
        let sources = [PolicySource::core(), PolicySource::standard_preset()];
        Pdp::layered(SCHEMA, &sources)
            .map(|layered| layered.pdp)
            .map_err(|e| PolicyError::Invalid(e.to_string()))
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
    /// `Deny`. A query Cedar cannot evaluate counts as not permitted: failure never permits.
    pub fn decide(&self, request: &PolicyRequest) -> Ruling {
        self.decide_checked(request).ruling
    }

    /// [`decide`](Self::decide), with the evaluation faults it met. A query that faulted counts as
    /// not permitted, so the ruling is never looser for it; `perform_unasked` faulting asks with
    /// `AskReason::EvaluationFailed`. The caller logs the faults with the task it knows.
    pub fn decide_checked(&self, request: &PolicyRequest) -> Ruled {
        crate::eval::decide(&self.policies, &self.schema, request)
    }
}
