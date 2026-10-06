//! Everything decided about a call before the first await: who acts, what the arguments are
//! and what they are worth, what standing consent and the task policy say, what Cedar rules,
//! and where the gate sends the call. Pure over the router's state and its clock.

use crate::argcheck::check_call;
use crate::consent::consent_for;
use crate::coverage::coverage;
use crate::gate::{GateInputs, Pending, gate};
use crate::labels::{
    arg_labels, args_confidentiality, label_args, planner_integrity, sink_integrity,
};
use crate::prepared::{
    Early, Prepared, digest, entities, grant_state, proposed, typed_history, usage_of,
};
use crate::router::Router;
use crate::seams::{Clock, GrantStore, Seams};
use crate::session::{CloseCause, SessionEffect, SessionEvent, SessionState, Taint, session_step};
use crate::spacing::relation_of;
use crate::state::{RouterState, SessionRecord};
use crate::who::Who;
use action_review::{GoalKey, ReviewRequest, repeated};
use docket_core::{
    ActionDecl, ActionGrant, BudgetKind, CallId, CallRefusal, CallRequest, Classification, Cost,
    Depth, Impact, Lasting, Reviewed, Ruling, Saw, WindowKey,
};
use policy_point::{
    ActionFacts, CoverageState, Op, PolicyContext, PolicyRequest, PrincipalFacts, SpaceRelation,
};
use porter_core::Count;
use porter_core::consent::Verdict;
use prov::{Effect, Integrity, SpaceId, SpaceScope};

/// Why a session takes no call: it is over, or the breaker paused it.
fn admit(record: &SessionRecord) -> Result<(), CallRefusal> {
    if let SessionState::Closed(cause) = record.state {
        return Err(match cause {
            CloseCause::WallExhausted => CallRefusal::OverBudget(BudgetKind::Wall),
            CloseCause::Closed | CloseCause::SpaceHalted => {
                CallRefusal::Halted(SpaceScope::Only(record.space.clone()))
            }
        });
    }
    match session_step(record.state, SessionEvent::Perform)
        .1
        .into_iter()
        .next()
    {
        Some(SessionEffect::Refuse(why)) => Err(why),
        _ => Ok(()),
    }
}

/// How much is at stake in a call: outbound or destructive, many things, or lasting memory.
fn impact_of(decl: &ActionDecl, count: Count, mass_at: Count) -> Impact {
    if decl.effect >= Effect::Outbound
        || count.0.saturating_mul(2) > mass_at.0
        || decl.lasting == Lasting::AgentMemory
    {
        Impact::High
    } else {
        Impact::Low
    }
}

/// What the router has worked out about a call before it asks Cedar.
struct Facts<'a> {
    who: &'a Who,
    decl: &'a ActionDecl,
    request: &'a CallRequest,
    record: &'a SessionRecord,
    consent: &'a Verdict,
    coverage: CoverageState,
    strictness: docket_core::Strictness,
    count: Count,
    impact: Impact,
    relation: SpaceRelation,
}

impl<S: Seams> Router<S> {
    /// The request Cedar rules on.
    fn policy_request(&self, f: &Facts<'_>) -> PolicyRequest {
        let saw = f.record.saw;
        let args = f
            .request
            .args
            .values()
            .map(|a| a.label.integrity)
            .chain([planner_integrity(&saw)])
            .min()
            .unwrap_or(Integrity::Trusted);
        PolicyRequest {
            principal: PrincipalFacts {
                kind: f.who.actor.kind(),
                caller: f.who.caller.app.name.clone(),
                isolation: f.who.caller.app.isolation,
            },
            op: Op::Perform,
            resource: ActionFacts {
                app: f.request.action.app.clone(),
                action: f.decl.name.clone(),
                effect: f.decl.effect,
                classes: f.decl.classes.clone(),
                reach: f.decl.reach,
                lasting: f.decl.lasting,
            },
            context: PolicyContext {
                space: f.record.space.clone(),
                target_space: f.relation,
                args,
                planner: planner_integrity(&saw),
                confidentiality: args_confidentiality(&f.request.args),
                usage: usage_of(&f.who.actor),
                origin: f.request.origin,
                count: f.count,
                mass_at: self.agent_config().mass_at,
                grant: grant_state(f.consent),
                terminal: crate::terminal::held(
                    f.record,
                    &f.request.action,
                    self.seams.clock().now(),
                ),
                coverage: f.coverage,
                task_ceiling: f.record.policy.as_ref().map_or(Effect::Read, |p| p.ceiling),
                strictness: f.strictness,
                saw,
                sinks: sink_integrity(f.decl, &f.request.args),
                impact: f.impact,
            },
        }
    }

    /// Decides everything about a call that needs no await. `Early` ends the call at once
    /// (an unknown action, bad arguments, a paused or closed session).
    pub(crate) fn prepare(
        &self,
        who: &Who,
        request: CallRequest,
        window: Option<WindowKey>,
        depth: Depth,
    ) -> Result<Prepared, Box<Early>> {
        self.prepare_as(None, None, who, request, window, depth)
    }

    /// `prepare` for a call that may have been classified: the call keeps `id` if it has one,
    /// and its effect, for Cedar, the budget, the taint rule and the audit, is the effect the
    /// classification used (never above the declared one).
    pub(crate) fn prepare_as(
        &self,
        id: Option<CallId>,
        class: Option<Classification>,
        who: &Who,
        request: CallRequest,
        window: Option<WindowKey>,
        depth: Depth,
    ) -> Result<Prepared, Box<Early>> {
        let now = self.seams.clock().now();
        let grants = self.seams.grants().grants();
        let mut st = self.locked();
        let id = id.unwrap_or_else(|| CallId(u64::from(st.mint())));
        let early = |effect: Effect, refusal: CallRefusal| {
            Box::new(Early {
                id,
                who: who.clone(),
                action: request.action.clone(),
                effect,
                space: SpaceId::desktop(),
                refusal,
            })
        };
        let Some(mut decl) = st.registry.action(&request.action).cloned() else {
            let why = CallRefusal::NoSuchAction(request.action.clone());
            return Err(early(Effect::Read, why));
        };
        if let Some(c) = &class {
            decl.effect = c.used.min(decl.effect);
        }
        match &who.session {
            None => Ok(self.prepare_person(id, who, request, decl, window, depth)),
            Some(session) => {
                let Some(record) = st.sessions.get(session) else {
                    return Err(early(decl.effect, CallRefusal::Halted(SpaceScope::Any)));
                };
                self.prepare_agent(
                    &st, record, id, who, request, decl, window, depth, now, &grants, class,
                )
            }
        }
    }

    /// A call an agent made: arguments labelled and checked, then consent, coverage, Cedar and
    /// the gate.
    #[allow(clippy::too_many_arguments)]
    fn prepare_agent(
        &self,
        st: &RouterState,
        record: &SessionRecord,
        id: CallId,
        who: &Who,
        request: CallRequest,
        decl: ActionDecl,
        window: Option<WindowKey>,
        depth: Depth,
        now: prov::UnixSeconds,
        grants: &[ActionGrant],
        classified: Option<Classification>,
    ) -> Result<Prepared, Box<Early>> {
        let space = record.space.clone();
        let end = |refusal: CallRefusal| {
            Box::new(Early {
                id,
                who: who.clone(),
                action: request.action.clone(),
                effect: decl.effect,
                space: space.clone(),
                refusal,
            })
        };
        admit(record).map_err(end)?;
        let args = label_args(record, &who.voice, request.args.clone())
            .and_then(|a| check_call(&decl, &request.action.app, &request.target, a))
            .map_err(|(param, why)| end(CallRefusal::BadArgs { param, why }))?;
        let request = CallRequest { args, ..request };
        let targets = entities(&request.target);
        let count = Count(u32::try_from(targets.len()).unwrap_or(u32::MAX));
        let taint = match (record.saw.untrusted, decl.effect) {
            (Saw::Seen, effect) if effect > Effect::Read => Taint::Tainted,
            _ => Taint::Clean,
        };
        let consent = consent_for(
            grants,
            &who.grant_caller(),
            &decl,
            &request.action.app,
            &space,
            usage_of(&who.actor),
            taint,
        );
        let labels = arg_labels(&request.args, record);
        let cover = coverage(record.policy.as_ref(), &decl, &request, &labels, now);
        let strictness = st
            .strictness
            .get(&space)
            .copied()
            .unwrap_or(self.agent_config().strictness);
        let impact = impact_of(&decl, count, self.agent_config().mass_at);
        let ruling = self.pdp.decide(&self.policy_request(&Facts {
            who,
            decl: &decl,
            request: &request,
            record,
            consent: &consent,
            coverage: CoverageState::from(&cover),
            strictness,
            count,
            impact,
            relation: relation_of(st, &space, &decl.on, &targets),
        }));
        let goal = GoalKey {
            app: request.action.app.clone(),
            action: decl.name.clone(),
            kind: targets.first().map(|e| e.kind.clone()),
        };
        let digest = digest(&targets, &request.args);
        let cost = Cost {
            effect: decl.effect,
            entities: count,
            depth,
            review: match ruling {
                Ruling::AllowJudged(_) => Reviewed::Yes,
                _ => Reviewed::No,
            },
        };
        let pending = gate(&GateInputs {
            halt: &st.kill,
            space: &space,
            ledger: &record.ledger,
            budget: &self.agent_config().budget,
            cost: &cost,
            now,
            consent: &consent,
            ruling: &ruling,
            impact,
            repeat: repeated(&record.breaker, &goal, digest),
        });
        Ok(Prepared {
            id,
            who: who.clone(),
            space: space.clone(),
            review: Some(ReviewRequest {
                space,
                strictness,
                turns: record.turns.clone(),
                proposed: proposed(&decl, &request, &targets),
                labels,
                task_policy: record.policy.clone(),
                history: typed_history(record),
            }),
            decl,
            request,
            ruling,
            pending,
            goal,
            digest,
            cost,
            window,
            targets,
            activation: None,
            classified,
        })
    }

    /// A call the person made through their own surface: the arguments are checked, and
    /// nothing else stands between it and the app (the app's own alerts apply).
    fn prepare_person(
        &self,
        id: CallId,
        who: &Who,
        request: CallRequest,
        decl: ActionDecl,
        window: Option<WindowKey>,
        depth: Depth,
    ) -> Prepared {
        let targets = entities(&request.target);
        let count = Count(u32::try_from(targets.len()).unwrap_or(u32::MAX));
        let (pending, request) = match check_call(
            &decl,
            &request.action.app,
            &request.target,
            request.args.clone(),
        ) {
            Ok(args) => (Pending::Run, CallRequest { args, ..request }),
            Err((param, why)) => (
                Pending::Refuse(CallRefusal::BadArgs { param, why }),
                request,
            ),
        };
        Prepared {
            id,
            who: who.clone(),
            space: SpaceId::desktop(),
            goal: GoalKey {
                app: request.action.app.clone(),
                action: decl.name.clone(),
                kind: targets.first().map(|e| e.kind.clone()),
            },
            digest: digest(&targets, &request.args),
            cost: Cost {
                effect: decl.effect,
                entities: count,
                depth,
                review: Reviewed::No,
            },
            decl,
            request,
            ruling: Ruling::AllowFinal(vec![]),
            pending,
            review: None,
            window,
            targets,
            activation: None,
            // The person's own calls are not gated, so they are not classified.
            classified: None,
        }
    }
}
