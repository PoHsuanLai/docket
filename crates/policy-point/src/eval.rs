//! Running the three Cedar queries for a request and mapping the answers onto a `Ruling`.
//! Every context value is the slug of its Rust type, so the policies read the words the wire
//! does; nothing here decides, it only translates.

use crate::fault::{EvalFault, FaultCause, Ruled};
use crate::request::{BudgetFact, Op, PolicyContext, PolicyRequest};
use cedar_policy::{
    Authorizer, Context, Decision, Entities, Entity, EntityId, EntityTypeName, EntityUid,
    PolicySet, Request, RestrictedExpression, Schema,
};
use docket_core::{ArgSink, AskReason, PolicyId, Ruling};
use prov::{Effect, Integrity};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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

/// "ok", or the slug of the budget that would be over.
fn budget_slug(budget: &BudgetFact) -> String {
    match budget {
        BudgetFact::Within => "ok".to_owned(),
        BudgetFact::Over(kind) => slug(kind),
    }
}

fn context_json(c: &PolicyContext) -> Value {
    json!({
        "space": c.space.as_str(),
        "target_space": slug(&c.target_space),
        "args": slug(&c.args),
        "planner": slug(&c.planner),
        "confidentiality": slug(&c.confidentiality),
        "usage": slug(&c.usage),
        "origin": slug(&c.origin),
        "count": number(&c.count),
        "mass_at": number(&c.mass_at),
        "grant": slug(&c.grant),
        "terminal": slug(&c.terminal),
        "coverage": slug(&c.coverage),
        "task_ceiling": slug(&c.task_ceiling),
        "saw_private": slug(&c.saw.private),
        "saw_untrusted": slug(&c.saw.untrusted),
        "sinks": {
            "recipient": slug(&c.sinks.recipient),
            "destination": slug(&c.sinks.destination),
            "body": slug(&c.sinks.body),
            "path": slug(&c.sinks.path),
        },
        "impact": slug(&c.impact),
        "halt": slug(&c.halt),
        "budget": budget_slug(&c.budget),
        "repeat": slug(&c.repeat),
        "limit": slug(&c.limit),
        "taint": slug(&c.taint),
        "run": slug(&c.run),
        "mode": c.mode.word(),
        "rows": {
            "inside": c.rows.inside.word(),
            "other": c.rows.other.word(),
            "send": c.rows.send.word(),
            "delete": c.rows.delete.word(),
            "screen": c.rows.screen.word(),
        },
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
fn entities(
    request: &PolicyRequest,
    schema: &Schema,
) -> Result<(EntityUid, EntityUid, Entities), FaultCause> {
    let facts = &request.resource;
    let named = || FaultCause::Entities;
    let caller = uid("Quire::Caller", request.principal.caller.as_str()).ok_or_else(named)?;
    let app = uid("Quire::App", facts.app.as_str()).ok_or_else(named)?;
    let act = uid("Quire::Act", facts.action.as_str()).ok_or_else(named)?;
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
    .map_err(|_| FaultCause::Entities)?;
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
    .map_err(|_| FaultCause::Entities)?;
    let app_entity = Entity::new_no_attrs(app, HashSet::new());
    let all = Entities::from_entities([caller_entity, act_entity, app_entity], Some(schema))
        .map_err(|_| FaultCause::Entities)?;
    Ok((caller, act, all))
}

/// What one query said.
struct Answer {
    decision: Decision,
    /// The `@id`s of the policies that determined it.
    ids: Vec<PolicyId>,
    /// The `@why` sentence of each of those that has one.
    whys: BTreeMap<PolicyId, String>,
}

/// One query. A query that Cedar could not set up, or whose policies raised an evaluation
/// error (a policy that errors is skipped, so a forbid could silently not apply), is a fault:
/// the caller treats it as not permitted.
fn query(
    policies: &PolicySet,
    schema: &Schema,
    request: &PolicyRequest,
    action: &'static str,
) -> Result<Answer, EvalFault> {
    let fault = |cause: FaultCause| EvalFault {
        query: action,
        cause,
    };
    let (caller, act, entities) = entities(request, schema).map_err(fault)?;
    let action_uid = uid("Quire::Action", action).ok_or_else(|| fault(FaultCause::Action))?;
    let context =
        Context::from_json_value(context_json(&request.context), Some((schema, &action_uid)))
            .map_err(|_| fault(FaultCause::Context))?;
    let cedar = Request::new(caller, action_uid, act, context, Some(schema))
        .map_err(|_| fault(FaultCause::Request))?;
    let response = Authorizer::new().is_authorized(&cedar, policies, &entities);
    let errors = response.diagnostics().errors().count();
    if errors > 0 {
        return Err(fault(FaultCause::Policies(errors)));
    }
    let decided: Vec<_> = response
        .diagnostics()
        .reason()
        .filter_map(|id| policies.policy(id))
        .filter_map(|p| {
            let id = PolicyId(p.annotation("id")?.to_owned());
            Some((id, p.annotation("why").map(str::to_owned)))
        })
        .collect();
    let ids = decided
        .iter()
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let whys = decided
        .into_iter()
        .filter_map(|(id, why)| Some((id, why?)))
        .collect();
    Ok(Answer {
        decision: response.decision(),
        ids,
        whys,
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

/// A forbid as the reason the person is asked: the typed reason of our own rules, else the rule's
/// own sentence (its `@why`), else the bare rule.
fn ask_reason(id: &PolicyId, c: &PolicyContext, whys: &BTreeMap<PolicyId, String>) -> AskReason {
    match id.0.as_str() {
        "rule-of-two" => AskReason::RuleOfTwo,
        "untrusted-sink" => {
            untrusted_sink(c).map_or_else(|| AskReason::Rule(id.clone()), AskReason::UntrustedSink)
        }
        "cross-space" => AskReason::CrossSpace,
        "mass" => AskReason::Mass(c.count),
        "ask-always" => AskReason::AskAlways,
        "first-use" => AskReason::FirstUse,
        "tainted-always-asks" => AskReason::Tainted,
        "limit-ask" => AskReason::AskAlways,
        "cli-asks" => AskReason::FromTerminal,
        "cli-destructive-asks" => AskReason::Effect(Effect::Destructive),
        "lasting-from-untrusted" => AskReason::LastingFromUntrusted,
        _ => whys.get(id).map_or_else(
            || AskReason::Rule(id.clone()),
            |why| AskReason::Said {
                rule: id.clone(),
                why: why.clone(),
            },
        ),
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

/// The queries of one request, with the faults they met.
struct Queries<'a> {
    policies: &'a PolicySet,
    schema: &'a Schema,
    request: &'a PolicyRequest,
    faults: Vec<EvalFault>,
}

impl Queries<'_> {
    /// Runs one query; a fault is kept and the answer is "not permitted, no ids".
    fn run(&mut self, action: &'static str) -> Option<Answer> {
        match query(self.policies, self.schema, self.request, action) {
            Ok(answer) => Some(answer),
            Err(fault) => {
                self.faults.push(fault);
                None
            }
        }
    }

    fn done(self, ruling: Ruling) -> Ruled {
        Ruled {
            ruling,
            faults: self.faults,
        }
    }
}

fn permitted(answer: &Option<Answer>) -> bool {
    matches!(answer, Some(a) if a.decision == Decision::Allow)
}

fn whys_of(answer: &Option<Answer>) -> BTreeMap<PolicyId, String> {
    answer.as_ref().map(|a| a.whys.clone()).unwrap_or_default()
}

fn ids_of(answer: Option<Answer>) -> Vec<PolicyId> {
    answer.map(|a| a.ids).unwrap_or_default()
}

/// The ruling for one request, and every fault met on the way. A fault never permits: a query
/// that faulted is not permitted, so it is a `Deny` for `perform` and the one-query operations,
/// an `Ask` (with [`AskReason::EvaluationFailed`]) for `perform_unasked`, and a reviewed allow
/// for `perform_unjudged`.
pub(crate) fn decide(policies: &PolicySet, schema: &Schema, request: &PolicyRequest) -> Ruled {
    let mut q = Queries {
        policies,
        schema,
        request,
        faults: Vec::new(),
    };
    if request.op != Op::Perform {
        let answer = q.run(match request.op {
            Op::Perform => "perform",
            Op::Search => "search",
            Op::Preview => "preview",
            Op::Suggest => "suggest",
            Op::ReadContext => "read_context",
            Op::Undo => "undo",
            Op::IndexPush => "index_push",
        });
        let ruling = if permitted(&answer) {
            Ruling::AllowFinal(ids_of(answer))
        } else {
            Ruling::Deny(ids_of(answer))
        };
        return q.done(ruling);
    }
    let perform = q.run("perform");
    if !permitted(&perform) {
        return q.done(Ruling::Deny(ids_of(perform)));
    }
    let unasked = q.run("perform_unasked");
    if !permitted(&unasked) {
        let c = &request.context;
        let faulted = q.faults.iter().any(|f| f.query == "perform_unasked");
        let whys = whys_of(&unasked);
        let mut reasons: Vec<AskReason> = ids_of(unasked)
            .iter()
            .map(|id| ask_reason(id, c, &whys))
            .collect();
        // Two rules can give one reason (`ask-always` and `limit-ask`): say it once.
        let mut seen = Vec::new();
        reasons.retain(|reason| {
            let new = !seen.contains(reason);
            seen.push(reason.clone());
            new
        });
        let why = if faulted {
            vec![AskReason::EvaluationFailed]
        } else if reasons.is_empty() {
            vec![cell_reason(request.resource.effect, c)]
        } else {
            reasons
        };
        return q.done(Ruling::Ask(why));
    }
    let unjudged = q.run("perform_unjudged");
    let ruling = if permitted(&unjudged) {
        Ruling::AllowFinal(ids_of(unjudged))
    } else {
        Ruling::AllowJudged(ids_of(unasked))
    };
    q.done(ruling)
}
