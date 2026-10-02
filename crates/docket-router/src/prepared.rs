//! What a call is once the router has looked at it: the data every later stage reads, and the
//! small pure facts derived from it (who the actor is, what the arguments hash to, what the
//! reviewer is shown).

use crate::gate::Pending;
use crate::state::SessionRecord;
use crate::who::Who;
use action_review::GoalKey;
use action_review::{ArgDigest, ArgView, CallEndKind, ProposedAction, ReviewRequest, TypedStep};
use docket_core::{
    ActionDecl, ActionRef, Args, CallId, CallRefusal, CallRequest, CharCount, Cost, GrantCaller,
    ParamName, Ruling, StepEnd, TargetValue, Value, WindowKey, size_of,
};
use policy_point::GrantState;
use porter_core::Count;
use porter_core::GrantId;
use porter_core::consent::{GrantScope, Usage, Verdict};
use prov::{Actor, AgentRole, Effect, EntityId, Integrity, SpaceId};

/// A call that ended before it reached the gate.
#[derive(Debug, Clone)]
pub(crate) struct Early {
    pub id: CallId,
    pub who: Who,
    pub action: ActionRef,
    pub effect: Effect,
    pub space: SpaceId,
    pub refusal: CallRefusal,
}

/// A call ready to be driven.
#[derive(Debug, Clone)]
pub(crate) struct Prepared {
    pub id: CallId,
    pub who: Who,
    pub space: SpaceId,
    pub decl: ActionDecl,
    pub request: CallRequest,
    pub ruling: Ruling,
    pub pending: Pending,
    pub goal: GoalKey,
    pub digest: ArgDigest,
    pub cost: Cost,
    pub review: Option<ReviewRequest>,
    pub window: Option<WindowKey>,
    pub targets: Vec<EntityId>,
}

pub(crate) fn entities(target: &TargetValue) -> Vec<EntityId> {
    match target {
        TargetValue::Entities(ids) => ids.clone(),
        TargetValue::Nothing | TargetValue::Text(_) | TargetValue::Files(_) => vec![],
    }
}

/// A stable hash of what a call is about: the same action, targets and argument values.
pub(crate) fn digest(targets: &[EntityId], args: &Args) -> ArgDigest {
    let values: std::collections::BTreeMap<&ParamName, &Value> =
        args.iter().map(|(k, v)| (k, &v.value)).collect();
    let bytes = serde_json::to_vec(&(targets, values)).unwrap_or_default();
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    });
    ArgDigest(hash)
}

pub(crate) fn grant_state(v: &Verdict) -> GrantState {
    match v {
        Verdict::Granted {
            scope: GrantScope::Always,
            ..
        } => GrantState::Always,
        Verdict::Granted {
            scope: GrantScope::Once,
            ..
        } => GrantState::Once,
        Verdict::Ask => GrantState::None,
        Verdict::Denied => GrantState::Denied,
    }
}

pub(crate) fn usage_of(actor: &Actor) -> Usage {
    match actor {
        Actor::Companion {
            role: AgentRole::Worker { .. } | AgentRole::Cua { .. },
            ..
        } => Usage::Background,
        _ => Usage::Interactive,
    }
}

/// The steps of a session as the reviewer is told them: codes only.
pub(crate) fn typed_history(record: &SessionRecord) -> Vec<TypedStep> {
    record
        .history
        .iter()
        .map(|s| TypedStep {
            call: s.call,
            action: s.action.clone(),
            effect: s.effect,
            end: match &s.end {
                StepEnd::Done { .. } => CallEndKind::Done,
                StepEnd::Refused(CallRefusal::Denied(_)) => CallEndKind::Denied,
                StepEnd::Unconfirmed(_) => CallEndKind::Unconfirmed,
                StepEnd::Refused(_) => CallEndKind::Failed,
            },
            verdict: None,
        })
        .collect()
}

pub(crate) fn proposed(
    decl: &ActionDecl,
    call: &CallRequest,
    targets: &[EntityId],
) -> ProposedAction {
    let view = |name: &ParamName| {
        call.args.get(name).map(|a| match a.label.integrity {
            Integrity::Trusted => ArgView::Trusted(a.value.clone()),
            Integrity::Untrusted => ArgView::Untrusted {
                from: a.label.sources.clone(),
                size: CharCount(match &a.value {
                    Value::Text(t) | Value::Url(t) => size_of(t).0,
                    _ => 0,
                }),
            },
        })
    };
    ProposedAction {
        app: call.action.app.clone(),
        action: decl.name.clone(),
        label: decl.label.clone(),
        effect: decl.effect,
        kinds: targets.iter().map(|e| e.kind.clone()).collect(),
        count: Count(u32::try_from(targets.len()).unwrap_or(u32::MAX)),
        args: decl
            .params
            .iter()
            .filter_map(|p| view(&p.name).map(|v| (p.name.clone(), p.sink, v)))
            .collect(),
        lasting: decl.lasting,
    }
}

/// What a grant the person gives with "always" is keyed by: one per data class, for the whole
/// app in this Space.
pub(crate) fn grants_for(
    prepared: &Prepared,
    id: GrantId,
    at: prov::UnixSeconds,
    caller: GrantCaller,
) -> Vec<docket_core::ActionGrant> {
    prepared
        .decl
        .classes
        .iter()
        .map(|class| porter_core::consent::Grant {
            id: id.clone(),
            key: docket_core::ActionGrantKey {
                caller: caller.clone(),
                owner: prepared.request.action.app.clone(),
                target: docket_core::GrantTarget::App,
                class: *class,
                usage: usage_of(&prepared.who.actor),
                space: prov::SpaceScope::Only(prepared.space.clone()),
            },
            decision: porter_core::consent::Decision::Allow,
            scope: GrantScope::Always,
            at,
        })
        .collect()
}
