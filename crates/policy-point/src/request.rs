//! What the router asks the policy point: a principal, an operation, the action, and the
//! context that Cedar's rules read. Every context field has a slug form in the schema.

use docket_core::{Impact, Origin, Saw, SessionSaw, SinkIntegrity, Strictness};
use porter_core::consent::Usage;
use porter_core::{AppName, Count, DataClass, Isolation};
use prov::{ActionName, ActorKind, Confidentiality, Effect, Integrity, SpaceId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Who asks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrincipalFacts {
    /// The kind of actor.
    pub kind: ActorKind,
    /// The app the caller is, when it is one.
    pub caller: AppName,
    /// How far the name can be trusted.
    pub isolation: Isolation,
}

/// What is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// Perform an action.
    Perform,
    /// Search.
    Search,
    /// Preview.
    Preview,
    /// Suggest parameter options.
    Suggest,
    /// Read context.
    ReadContext,
    /// Undo.
    Undo,
    /// Push to the index.
    IndexPush,
}

/// What the action is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionFacts {
    /// The app that declares it.
    pub app: AppName,
    /// Its name.
    pub action: ActionName,
    /// Its effect.
    pub effect: Effect,
    /// The data classes it touches.
    pub classes: BTreeSet<DataClass>,
    /// Whether an agent may see it.
    pub reach: docket_core::AgentReach,
    /// Whether it writes lasting memory.
    pub lasting: docket_core::Lasting,
}

/// Where the target is relative to the session's Space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpaceRelation {
    /// The same Space.
    Same,
    /// Another one.
    Other,
    /// Not bound to any.
    Unbound,
}

/// What standing consent says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantState {
    /// An `Always` grant covers it.
    Always,
    /// Ask once.
    Once,
    /// No grant yet.
    None,
    /// Denied.
    Denied,
}

/// Whether the call is inside the task policy: Cedar's two-state view of `Coverage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageState {
    /// Inside.
    Inside,
    /// Outside.
    Outside,
}

impl From<&docket_core::Coverage> for CoverageState {
    fn from(coverage: &docket_core::Coverage) -> Self {
        match coverage {
            docket_core::Coverage::Inside => CoverageState::Inside,
            docket_core::Coverage::Outside(_) => CoverageState::Outside,
        }
    }
}

/// Everything about the call that Cedar's rules read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyContext {
    /// The session's Space.
    pub space: SpaceId,
    /// Where the target is.
    pub target_space: SpaceRelation,
    /// The integrity of the arguments.
    pub args: Integrity,
    /// The planner's integrity.
    pub planner: Integrity,
    /// How confidential what the call carries is.
    pub confidentiality: Confidentiality,
    /// Interactive or background.
    pub usage: Usage,
    /// Where the call came from.
    pub origin: Origin,
    /// How many things it touches.
    pub count: Count,
    /// More than this many asks (`agent.mass_at`, proposed 20).
    pub mass_at: Count,
    /// What standing consent says.
    pub grant: GrantState,
    /// Whether the call is inside the task policy.
    pub coverage: CoverageState,
    /// The task policy's effect ceiling.
    pub task_ceiling: Effect,
    /// The Space's strictness.
    pub strictness: Strictness,
    /// What the session has seen, computed by the router.
    pub saw: SessionSaw,
    /// The integrity of each sink.
    pub sinks: SinkIntegrity,
    /// How much is at stake.
    pub impact: Impact,
}

/// One question for the policy point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyRequest {
    /// Who asks.
    pub principal: PrincipalFacts,
    /// What is asked.
    pub op: Op,
    /// The action.
    pub resource: ActionFacts,
    /// What the rules read.
    pub context: PolicyContext,
}

impl PolicyContext {
    /// Whether the session has seen both private data and untrusted input: two legs of the Rule
    /// of Two.
    pub fn two_legs(&self) -> bool {
        self.saw.private == Saw::Seen && self.saw.untrusted == Saw::Seen
    }
}
