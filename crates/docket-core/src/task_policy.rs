//! The per-task policy: a deterministic narrowing, derived from the person's own words, inside
//! their standing grants.
//!
//! A grant ([`ActionGrant`](crate::ActionGrant)) says which app, class and Space the companion
//! may touch at all and lives across tasks. A `TaskPolicy` narrows that for one task and expires
//! with it. Both must allow an action. Narrowing is automatic and silent; widening needs a
//! confirmation that quotes the person's turn. No model can mint or widen a policy: not the
//! reviewer, the planner, the reader or the computer-use model. If the writer fails there is no
//! policy and every non-read call is outside.

use crate::call::CallRequest;
use crate::ids::{ActionRef, FileRef, LabelText, TurnId};
use crate::manifest::ArgSink;
use crate::planner::{ActionCard, UserTurn};
use crate::review::ReviewError;
use crate::value::Value;
use porter_core::{AppName, Count};
use prov::{Effect, EntityId, EntityKind, Integrity, Label, SpaceId, TaskId, UnixSeconds};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;

/// What one task may do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskPolicy {
    /// The task.
    pub task: TaskId,
    /// Its Space.
    pub space: SpaceId,
    /// The router-held user turns it comes from: only the person's own words.
    pub from: Vec<TurnId>,
    /// The actions it covers.
    pub actions: BTreeSet<ActionMatch>,
    /// The kinds of thing it covers.
    pub kinds: BTreeSet<EntityKind>,
    /// The highest effect it covers.
    pub ceiling: Effect,
    /// The most things one call may touch.
    pub max_count: Count,
    /// Recipients that trace to the person.
    pub recipients: Vec<TrustedPattern>,
    /// Destinations that trace to the person.
    pub destinations: Vec<TrustedPattern>,
    /// Paths that trace to the person.
    pub paths: Vec<TrustedPattern>,
    /// When it lapses (`agent.task_policy.max_s`, proposed 3600, or at task end).
    pub expires: UnixSeconds,
    /// Shown to the person when it widens.
    pub rationale: LabelText,
    /// Where it stands.
    pub state: TaskPolicyState,
}

/// What a policy covers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ActionMatch {
    /// This action.
    One(ActionRef),
    /// Any action of this app up to this effect.
    AppUpTo(AppName, Effect),
}

/// A value that traces to a person's turn or to something they chose; never content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum TrustedPattern {
    /// Exactly this value.
    Exact(Value),
    /// This thing.
    Entity(EntityId),
    /// This domain and its subdomains, by whole labels.
    Domain(String),
    /// Anything under this path.
    Under(FileRef),
}

/// Where a policy stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPolicyState {
    /// In force.
    Active,
    /// Replaced by a narrower or equal one.
    Superseded,
    /// Lapsed.
    Expired,
    /// Withdrawn in Settings or the control centre.
    Revoked,
}

/// How a new policy compares with the old.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum PolicyChange {
    /// Covers less: applied silently.
    Narrows,
    /// Covers the same.
    Same,
    /// Covers more: the person confirms.
    Widens(Vec<Widening>),
}

/// One way a policy would widen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Widening {
    /// A new action.
    Action(ActionMatch),
    /// A new kind.
    Kind(EntityKind),
    /// A higher effect ceiling.
    Ceiling(Effect),
    /// A larger count.
    Count(Count),
    /// A new trusted value for a sink.
    Pattern(ArgSink, TrustedPattern),
    /// A later expiry.
    Expiry,
}

/// Whether a call is inside a policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Coverage {
    /// Covered.
    Inside,
    /// Not covered, and this is what would have to widen.
    Outside(Widening),
}

/// The integrity of each sink an argument feeds: what the untrusted-sink rule reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SinkIntegrity {
    /// The recipient.
    pub recipient: Integrity,
    /// The destination.
    pub destination: Integrity,
    /// The body.
    pub body: Integrity,
    /// The path.
    pub path: Integrity,
}

/// Whether a session has seen something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Saw {
    /// It has.
    Seen,
    /// It has not.
    NotSeen,
}

/// What a session has seen: computed by the router, never reported by a model. It only grows,
/// and resets when the task ends. *Private* means the router delivered, plainly or as a handle
/// the reader resolved, a value with `Private` or `Secret` confidentiality. *Untrusted* means a
/// value with `Untrusted` integrity reached the planner or the reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionSaw {
    /// Private or secret data.
    pub private: Saw,
    /// Untrusted content.
    pub untrusted: Saw,
}

/// What the router knows about the arguments of one call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArgLabels {
    /// The label of each argument.
    pub per_arg: BTreeMap<crate::ids::ParamName, Label>,
    /// The planner's integrity.
    pub planner: Integrity,
    /// What the session has seen.
    pub saw: SessionSaw,
}

/// How a new policy compares with the old: pure and table-tested.
pub fn compare(new: &TaskPolicy, old: &TaskPolicy) -> PolicyChange {
    let _ = (new, old);
    todo!("compare: actions, kinds, ceiling, count, patterns and expiry, each against the old")
}

/// Whether a call is inside a policy. An untrusted sink argument is never inside.
pub fn covers(policy: &TaskPolicy, call: &CallRequest, labels: &ArgLabels) -> Coverage {
    let _ = (policy, call, labels);
    todo!(
        "covers: action match, kind, ceiling, count, and every sink argument trusted or matching a pattern"
    )
}

/// Derives a policy from the person's turns alone. Its input is the router-held turns and the
/// action catalogue; it never sees content, tool output or planner prose.
pub trait PolicyWriter: Send + Sync {
    /// The policy for these turns in this Space, or why there is none. The router stamps the
    /// result's `from` and `expires`; `task` is the task it is for.
    fn derive(
        &self,
        task: &TaskId,
        turns: &[UserTurn],
        catalogue: &[ActionCard],
        space: &SpaceId,
    ) -> impl Future<Output = Result<TaskPolicy, ReviewError>> + Send;
}
