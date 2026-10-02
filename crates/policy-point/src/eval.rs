//! Running the three Cedar queries for a request and mapping the answers onto a `Ruling`.
//! Every context value is the slug of its Rust type, so the policies read the words the wire
//! does; nothing here decides, it only translates.

use crate::request::{Op, PolicyContext, PolicyRequest};
use cedar_policy::{
    Authorizer, Context, Decision, Entities, Entity, EntityId, EntityTypeName, EntityUid,
    PolicySet, Request, RestrictedExpression, Schema,
};
use docket_core::{ArgSink, AskReason, PolicyId, Ruling};
use prov::{Effect, Integrity};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::str::FromStr;

/// The serde slug of a closed set: a bare string, or the `kind` of a tagged one.
fn slug<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(s)) => s,
        Ok(Value::Object(map)) => match map.get("kind") {
            Some(Value::String(kind)) => kind.clone(),
            _ => String::new(),
        },
        Ok(_) | Err(_) => String::new(),
    }
}

fn number<T: Serialize>(value: &T) -> u64 {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
}

fn context_json(c: &PolicyContext) -> Value {
    json!({
        "target_space": slug(&c.target_space),
        "args": slug(&c.args),
        "planner": slug(&c.planner),
        "confidentiality": slug(&c.confidentiality),
        "usage": slug(&c.usage),
        "origin": slug(&c.origin),
        "count": number(&c.count),
        "mass_at": number(&c.mass_at),
        "grant": slug(&c.grant),
        "coverage": slug(&c.coverage),
        "task_ceiling": slug(&c.task_ceiling),
        "strictness": slug(&c.strictness),
        "saw_private": slug(&c.saw.private),
        "saw_untrusted": slug(&c.saw.untrusted),
        "sinks": {
            "recipient": slug(&c.sinks.recipient),
            "destination": slug(&c.sinks.destination),
            "body": slug(&c.sinks.body),
            "path": slug(&c.sinks.path),
        },
        "impact": slug(&c.impact),
    })
}

fn uid(type_name: &str, id: &str) -> Option<EntityUid> {
    let ty = EntityTypeName::from_str(type_name).ok()?;
    Some(EntityUid::from_type_name_and_id(ty, EntityId::new(id)))
}

fn string(value: String) -> RestrictedExpression {
    RestrictedExpression::new_string(value)
}

/// The entities of one request: the caller, the action's app and the action itself.
fn entities(request: &PolicyRequest, schema: &Schema) -> Option<(EntityUid, EntityUid, Entities)> {
    let facts = &request.resource;
    let caller = uid("Quire::Caller", request.principal.caller.as_str())?;
    let app = uid("Quire::App", facts.app.as_str())?;
    let act = uid("Quire::Act", facts.action.as_str())?;
    let classes = facts
        .classes
        .iter()
        .map(|class| string(slug(class)))
        .collect::<Vec<_>>();
    let caller_entity = Entity::new(
        caller.clone(),
        HashMap::from([("kind".to_owned(), string(slug(&request.principal.kind)))]),
        HashSet::new(),
    )
    .ok()?;
    let act_entity = Entity::new(
        act.clone(),
        HashMap::from([
            ("effect".to_owned(), string(slug(&facts.effect))),
            ("classes".to_owned(), RestrictedExpression::new_set(classes)),
            ("reach".to_owned(), string(slug(&facts.reach))),
            ("lasting".to_owned(), string(slug(&facts.lasting))),
        ]),
        HashSet::from([app.clone()]),
    )
    .ok()?;
    let app_entity = Entity::new_no_attrs(app, HashSet::new());
    let all =
        Entities::from_entities([caller_entity, act_entity, app_entity], Some(schema)).ok()?;
    Some((caller, act, all))
}

/// What one query said.
struct Answer {
    decision: Decision,
    /// The `@id`s of the policies that determined it.
    ids: Vec<PolicyId>,
}

fn query(
    policies: &PolicySet,
    schema: &Schema,
    request: &PolicyRequest,
    action: &str,
) -> Option<Answer> {
    let (caller, act, entities) = entities(request, schema)?;
    let action_uid = uid("Quire::Action", action)?;
    let context =
        Context::from_json_value(context_json(&request.context), Some((schema, &action_uid)))
            .ok()?;
    let cedar = Request::new(caller, action_uid, act, context, Some(schema)).ok()?;
    let response = Authorizer::new().is_authorized(&cedar, policies, &entities);
    let ids = response
        .diagnostics()
        .reason()
        .filter_map(|id| policies.policy(id))
        .filter_map(|p| p.annotation("id"))
        .map(|id| PolicyId(id.to_owned()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Some(Answer {
        decision: response.decision(),
        ids,
    })
}

/// The first untrusted sink, in the order recipient, destination, path, body.
fn untrusted_sink(c: &PolicyContext) -> Option<ArgSink> {
    [
        (ArgSink::Recipient, c.sinks.recipient),
        (ArgSink::Destination, c.sinks.destination),
        (ArgSink::Path, c.sinks.path),
        (ArgSink::Body, c.sinks.body),
    ]
    .into_iter()
    .find(|(_, integrity)| *integrity == Integrity::Untrusted)
    .map(|(sink, _)| sink)
}

/// A named forbid as the reason the person is asked.
fn ask_reason(id: &PolicyId, c: &PolicyContext) -> AskReason {
    match id.0.as_str() {
        "rule-of-two" => AskReason::RuleOfTwo,
        "untrusted-sink" => {
            untrusted_sink(c).map_or_else(|| AskReason::Rule(id.clone()), AskReason::UntrustedSink)
        }
        "cross-space" => AskReason::CrossSpace,
        "mass" => AskReason::Mass(c.count),
        "ask-always" => AskReason::AskAlways,
        "first-use" => AskReason::FirstUse,
        "lasting-from-untrusted" => AskReason::LastingFromUntrusted,
        _ => AskReason::Rule(id.clone()),
    }
}

/// The reason for a cell of the grid that asks with no forbid naming it.
fn cell_reason(effect: Effect, c: &PolicyContext) -> AskReason {
    let outside = matches!(c.coverage, crate::request::CoverageState::Outside);
    let tainted = c.planner == Integrity::Untrusted || untrusted_sink(c).is_some();
    match (effect, outside, tainted) {
        (Effect::Read, _, _) => AskReason::Effect(effect),
        (_, true, _) => AskReason::OutsideTask,
        (_, false, true) => AskReason::Tainted,
        (_, false, false) => AskReason::Effect(effect),
    }
}

fn permitted(answer: &Option<Answer>) -> bool {
    matches!(answer, Some(a) if a.decision == Decision::Allow)
}

fn ids_of(answer: Option<Answer>) -> Vec<PolicyId> {
    answer.map(|a| a.ids).unwrap_or_default()
}

/// The ruling for one request.
pub(crate) fn decide(policies: &PolicySet, schema: &Schema, request: &PolicyRequest) -> Ruling {
    let run = |action: &str| query(policies, schema, request, action);
    if request.op != Op::Perform {
        let answer = run(match request.op {
            Op::Perform => "perform",
            Op::Search => "search",
            Op::Preview => "preview",
            Op::Suggest => "suggest",
            Op::ReadContext => "read_context",
            Op::Undo => "undo",
            Op::IndexPush => "index_push",
        });
        return if permitted(&answer) {
            Ruling::AllowFinal(ids_of(answer))
        } else {
            Ruling::Deny(ids_of(answer))
        };
    }
    let perform = run("perform");
    if !permitted(&perform) {
        return Ruling::Deny(ids_of(perform));
    }
    let unasked = run("perform_unasked");
    if !permitted(&unasked) {
        let c = &request.context;
        let reasons: Vec<AskReason> = ids_of(unasked).iter().map(|id| ask_reason(id, c)).collect();
        return Ruling::Ask(if reasons.is_empty() {
            vec![cell_reason(request.resource.effect, c)]
        } else {
            reasons
        });
    }
    let unjudged = run("perform_unjudged");
    if permitted(&unjudged) {
        Ruling::AllowFinal(ids_of(unjudged))
    } else {
        Ruling::AllowJudged(ids_of(unasked))
    }
}
