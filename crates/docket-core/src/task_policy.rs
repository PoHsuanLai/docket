//! The per-task policy: a deterministic narrowing, derived from the person's own words, inside
//! their standing grants.
//!
//! A grant ([`ActionGrant`](crate::ActionGrant)) says which app, class and Space the companion
//! may touch at all and lives across tasks. A `TaskPolicy` narrows that for one task and expires
//! with it. Both must allow an action. Narrowing is automatic and silent; widening needs a
//! confirmation that quotes the person's turn. No model can mint or widen a policy: not the
//! reviewer, the planner, the reader or the computer-use model. If the writer fails there is no
//! policy and every non-read call is outside.

use crate::pattern::{pattern_inside, pattern_matches};
use docket_manifest::{ActionDecl, ArgSink};
use docket_vocab::{ActionRef, CallRequest, FileRef, LabelText, TargetValue, TurnId, Value};
use porter_core::{AppName, Count};
use prov::{Effect, EntityId, EntityKind, Integrity, Label, SpaceId, TaskId, UnixSeconds};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

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
    /// Paths that trace to the person. For a call whose target is files, a non-empty list bounds
    /// every file (whole path components, as a standing grant's scope does) and an empty list
    /// leaves the files to the session's directory and the grants. For an untrusted argument that
    /// feeds a path sink, an empty list trusts nothing.
    pub paths: Vec<TrustedPattern>,
    /// When it lapses (`TASK_POLICY_MAX`, or at task end).
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
    pub per_arg: BTreeMap<docket_vocab::ParamName, Label>,
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
    // A list that bounded files and is now empty bounds nothing: files anywhere.
    let unbounded = (new.paths.is_empty() && !old.paths.is_empty())
        .then(|| anywhere().map(|p| Widening::Pattern(ArgSink::Path, p)))
        .flatten();
    actions
        .chain(kinds)
        .chain(ceiling)
        .chain(count)
        .chain(patterns)
        .chain(unbounded)
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
        paths: bounded_paths(&a.paths, &b.paths),
        expires: a.expires.min(b.expires),
        rationale: a.rationale.clone(),
        state: if both_active {
            a.state
        } else {
            TaskPolicyState::Revoked
        },
    }
}

/// The pattern that matches every file.
fn anywhere() -> Option<TrustedPattern> {
    FileRef::parse("/").ok().map(TrustedPattern::Under)
}

/// What both path lists allow. An empty list does not bound files, so it yields to the other
/// side's; two lists keep what each covers of the other.
fn bounded_paths(a: &[TrustedPattern], b: &[TrustedPattern]) -> Vec<TrustedPattern> {
    match (a.is_empty(), b.is_empty()) {
        (true, _) => b.to_vec(),
        (_, true) => a.to_vec(),
        (false, false) => a
            .iter()
            .filter(|p| b.iter().any(|o| pattern_inside(p, o)))
            .chain(b.iter().filter(|p| a.iter().any(|o| pattern_inside(p, o))))
            .fold(Vec::new(), |mut kept, p| {
                if !kept.contains(p) {
                    kept.push(p.clone());
                }
                kept
            }),
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
    judge(policy, decl, call, labels, Judged::Task)
}

/// Whether the call is inside a helper's reach: as [`covers`], except that words feeding a body
/// are inside, and so are the companion actions every helper keeps (handing a job on, suggesting
/// a routine, loading a skill), which no card names. The person set on the card what the helper may change in each app; what it writes
/// there stays in the app, and a session that has read untrusted content still asks by the taint
/// rule. Where things go (recipients, destinations, paths) is judged as for any policy.
pub fn covers_reach(
    reach: &TaskPolicy,
    decl: &ActionDecl,
    call: &CallRequest,
    labels: &ArgLabels,
) -> Coverage {
    judge(reach, decl, call, labels, Judged::Reach)
}

/// What the policy is judged as.
#[derive(Debug, Clone, Copy)]
enum Judged {
    /// A task's policy: untrusted words feeding a body are outside, no trusted pattern can vouch
    /// for them.
    Task,
    /// A helper's reach: body words are inside (they stay in the app the reach names), and so are
    /// the companion actions a helper keeps.
    Reach,
}

fn judge(
    policy: &TaskPolicy,
    decl: &ActionDecl,
    call: &CallRequest,
    labels: &ArgLabels,
    judged: Judged,
) -> Coverage {
    if policy.state != TaskPolicyState::Active {
        return Coverage::Outside(Widening::Expiry);
    }
    if decl.effect > policy.ceiling {
        return Coverage::Outside(Widening::Ceiling(decl.effect));
    }
    let kept = matches!(judged, Judged::Reach) && crate::reach_menu::kept_by_helpers(&call.action);
    let named = kept
        || policy.actions.iter().any(|m| match m {
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
    if let Some(file) = file_outside(&policy.paths, &call.target) {
        return Coverage::Outside(Widening::Pattern(
            ArgSink::Path,
            TrustedPattern::Exact(Value::File(file.clone())),
        ));
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
            match (sink, judged) {
                (ArgSink::Body, Judged::Reach) => None,
                _ => uncovered_argument(policy, sink, &arg.value),
            }
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

/// The first file target that no path of the policy covers; none when the policy names no paths
/// or the target is not files.
fn file_outside<'a>(paths: &[TrustedPattern], target: &'a TargetValue) -> Option<&'a FileRef> {
    let TargetValue::Files(files) = target else {
        return None;
    };
    if paths.is_empty() {
        return None;
    }
    files.iter().find(|f| {
        let value = Value::File((*f).clone());
        !paths.iter().any(|p| pattern_matches(p, &value))
    })
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
