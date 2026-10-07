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
use crate::manifest::{ActionDecl, ArgSink};
use crate::pattern::{pattern_inside, pattern_matches};
use crate::planner::{ActionCard, UserTurn};
use crate::review::ReviewError;
use crate::value::{TargetValue, Value};
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

/// How a new policy compares with the old: pure and table-tested. It widens when it covers
/// anything the old did not (the first such things are listed), narrows when only the old
/// covered something, and is the same otherwise. The task, turns, rationale and state are
/// not coverage.
pub fn compare(new: &TaskPolicy, old: &TaskPolicy) -> PolicyChange {
    let wider = widenings(new, old);
    if !wider.is_empty() {
        PolicyChange::Widens(wider)
    } else if widenings(old, new).is_empty() {
        PolicyChange::Same
    } else {
        PolicyChange::Narrows
    }
}

/// What `new` covers that `old` does not.
fn widenings(new: &TaskPolicy, old: &TaskPolicy) -> Vec<Widening> {
    let actions = new
        .actions
        .iter()
        .filter(|m| !action_inside(m, new.ceiling, &old.actions))
        .cloned()
        .map(Widening::Action);
    let kinds = new
        .kinds
        .difference(&old.kinds)
        .cloned()
        .map(Widening::Kind);
    let ceiling = (new.ceiling > old.ceiling).then_some(Widening::Ceiling(new.ceiling));
    let count = (new.max_count > old.max_count).then_some(Widening::Count(new.max_count));
    let patterns = [
        (ArgSink::Recipient, &new.recipients, &old.recipients),
        (ArgSink::Destination, &new.destinations, &old.destinations),
        (ArgSink::Path, &new.paths, &old.paths),
    ]
    .into_iter()
    .flat_map(|(sink, new, old)| {
        new.iter()
            .filter(|p| !old.iter().any(|o| pattern_inside(p, o)))
            .map(move |p| Widening::Pattern(sink, p.clone()))
    });
    let expiry = (new.expires > old.expires).then_some(Widening::Expiry);
    actions
        .chain(kinds)
        .chain(ceiling)
        .chain(count)
        .chain(patterns)
        .chain(expiry)
        .collect()
}

/// What both `a` and `b` allow, under `a`'s task, turns and rationale: the lower ceiling, count
/// and expiry, the kinds both name, and the actions and trusted patterns each side holds that
/// the other covers. It never covers more than either (`compare` says `Narrows` or `Same`
/// against both), and a policy in another Space shares nothing: Spaces are walls. It is the
/// derivation of a child task's policy from its parent's.
pub fn intersection(a: &TaskPolicy, b: &TaskPolicy) -> TaskPolicy {
    let ceiling = a.ceiling.min(b.ceiling);
    let shared_space = a.space == b.space;
    let both_active = a.state == TaskPolicyState::Active && b.state == TaskPolicyState::Active;
    let live = |keep: bool| keep && shared_space && both_active;
    let actions = a
        .actions
        .iter()
        .filter(|m| action_inside(m, ceiling, &b.actions))
        .chain(
            b.actions
                .iter()
                .filter(|m| action_inside(m, ceiling, &a.actions)),
        )
        .cloned()
        .collect::<BTreeSet<_>>();
    let patterns = |a: &[TrustedPattern], b: &[TrustedPattern]| {
        let mut kept: Vec<TrustedPattern> = Vec::new();
        for p in a
            .iter()
            .filter(|p| b.iter().any(|o| pattern_inside(p, o)))
            .chain(b.iter().filter(|p| a.iter().any(|o| pattern_inside(p, o))))
        {
            if !kept.contains(p) {
                kept.push(p.clone());
            }
        }
        kept
    };
    TaskPolicy {
        task: a.task.clone(),
        space: a.space.clone(),
        from: a.from.clone(),
        actions: if live(true) { actions } else { BTreeSet::new() },
        kinds: if live(true) {
            a.kinds.intersection(&b.kinds).cloned().collect()
        } else {
            BTreeSet::new()
        },
        ceiling,
        max_count: a.max_count.min(b.max_count),
        recipients: patterns(&a.recipients, &b.recipients),
        destinations: patterns(&a.destinations, &b.destinations),
        paths: patterns(&a.paths, &b.paths),
        expires: a.expires.min(b.expires),
        rationale: a.rationale.clone(),
        state: if both_active {
            a.state
        } else {
            TaskPolicyState::Revoked
        },
    }
}

/// Whether `m`, bounded by the policy's `ceiling`, is already covered by one of `old`.
fn action_inside(m: &ActionMatch, ceiling: Effect, old: &BTreeSet<ActionMatch>) -> bool {
    let (app, reach) = match m {
        ActionMatch::One(a) => (&a.app, ceiling),
        ActionMatch::AppUpTo(app, e) => (app, (*e).min(ceiling)),
    };
    old.contains(m)
        || old
            .iter()
            .any(|o| matches!(o, ActionMatch::AppUpTo(a, e) if a == app && reach <= *e))
}

/// The trusted patterns a sink may be matched against: recipients, destinations (a query leaves
/// the machine like a destination) and paths. A body has none: untrusted content never becomes
/// trusted by what it says.
fn patterns_for(policy: &TaskPolicy, sink: ArgSink) -> &[TrustedPattern] {
    match sink {
        ArgSink::Recipient => &policy.recipients,
        ArgSink::Destination | ArgSink::Query => &policy.destinations,
        ArgSink::Path => &policy.paths,
        ArgSink::Inert | ArgSink::Body => &[],
    }
}

/// Whether the call is inside the task policy, judged from the manifest's declaration of the
/// action (its effect and where each argument goes), the call and its labels: the policy is in
/// force, the action's effect is within the ceiling and within the cap of an app-wide grant, the
/// action and every target kind are named, no more things are touched than allowed, and every
/// argument that feeds a sink is either trusted or matches a trusted pattern for that sink. An
/// argument the manifest does not declare is treated as a body. Expiry by the clock is the
/// router's.
pub fn covers(
    policy: &TaskPolicy,
    decl: &ActionDecl,
    call: &CallRequest,
    labels: &ArgLabels,
) -> Coverage {
    if policy.state != TaskPolicyState::Active {
        return Coverage::Outside(Widening::Expiry);
    }
    if decl.effect > policy.ceiling {
        return Coverage::Outside(Widening::Ceiling(decl.effect));
    }
    let named = policy.actions.iter().any(|m| match m {
        ActionMatch::One(a) => *a == call.action,
        ActionMatch::AppUpTo(app, cap) => *app == call.action.app && decl.effect <= *cap,
    });
    if !named {
        return Coverage::Outside(Widening::Action(ActionMatch::One(call.action.clone())));
    }
    let targets = target_entities(&call.target);
    if let Some(kind) = targets
        .iter()
        .map(|e| &e.kind)
        .find(|k| !policy.kinds.contains(*k))
    {
        return Coverage::Outside(Widening::Kind(kind.clone()));
    }
    let count = Count(u32::try_from(targets.len()).unwrap_or(u32::MAX));
    if count > policy.max_count {
        return Coverage::Outside(Widening::Count(count));
    }
    // An argument the router did not label is untrusted: the planner never vouches for itself.
    call.args
        .iter()
        .filter(|(name, _)| {
            labels
                .per_arg
                .get(*name)
                .is_none_or(|l| l.integrity == Integrity::Untrusted)
        })
        .find_map(|(name, arg)| {
            let sink = decl
                .params
                .iter()
                .find(|p| &p.name == name)
                .map_or(ArgSink::Body, |p| p.sink);
            uncovered_argument(policy, sink, &arg.value)
        })
        .map_or(Coverage::Inside, Coverage::Outside)
}

/// The widening that would make an untrusted `value` feeding `sink` acceptable, or `None` when
/// the sink is inert or a trusted pattern of the policy already matches it.
fn uncovered_argument(policy: &TaskPolicy, sink: ArgSink, value: &Value) -> Option<Widening> {
    if sink == ArgSink::Inert
        || patterns_for(policy, sink)
            .iter()
            .any(|p| pattern_matches(p, value))
    {
        return None;
    }
    Some(Widening::Pattern(
        sink,
        TrustedPattern::Exact(value.clone()),
    ))
}

fn target_entities(target: &TargetValue) -> Vec<&EntityId> {
    match target {
        TargetValue::Entities(ids) => ids.iter().collect(),
        TargetValue::Nothing
        | TargetValue::Handles(_)
        | TargetValue::Text(_)
        | TargetValue::Files(_) => vec![],
    }
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
    ) -> impl Future<Output = Result<crate::Derived, ReviewError>> + Send;
}
