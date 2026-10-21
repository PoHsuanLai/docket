//! Everything decided about a call before the first await: who acts, what the arguments are
//! and what they are worth, what standing consent and the task policy say, what Cedar rules,
//! and where the gate sends the call. Pure over the router's state and its clock.

use crate::argcheck::{check_call, no_handle_names, target_name};
use crate::consent::{consent_for, within_reach};
use crate::coverage::coverage;
use crate::facts::{RunFacts, budget_fact, repeat_fact, run_facts, taint_fact};
use crate::gate::{GateInputs, Pending, gate};
use crate::labels::{
    arg_labels, args_confidentiality, label_args, planner_integrity, resolve_target, sink_integrity,
};
use crate::owned::{owned_by_another, touched};
use crate::prepared::{
    Early, Prepared, digest, entities, grant_state, proposed, typed_history, usage_of,
};
use crate::router::Router;
use crate::seams::{Clock, GrantStore, Seams};
use crate::session::{CloseCause, SessionEffect, SessionEvent, SessionState, session_step};
use crate::spacing::relation_of;
use crate::standing::StandingCtx;
use crate::state::{RouterState, SessionRecord};
use crate::turn_start::{TurnStart, pass, rule};
use crate::who::Who;
use action_review::{GoalKey, RepeatState, ReviewRequest, repeated};
use docket_core::{
    ActionDecl, ActionGrant, BreakerState, BudgetKind, BudgetState, CallId, CallRefusal,
    CallRequest, CallerRole, Classification, Cost, DenyCode, Depth, Impact, Lasting, LimitCall,
    Limited, Mode, ModeRows, Reviewed, Ruling, SpaceAccess, WindowKey, is_agent_action, space_access,
};
use policy_point::{
    ActionFacts, BudgetFact, CoverageState, Op, PolicyContext, PolicyRequest, PrincipalFacts,
    SpaceRelation, TaintFact,
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
    count: Count,
    impact: Impact,
    relation: SpaceRelation,
    run: &'a RunFacts,
    repeat: RepeatState,
    taint: TaintFact,
    mode: Mode,
    rows: ModeRows,
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
                saw,
                sinks: sink_integrity(f.decl, &f.request.args),
                impact: f.impact,
                halt: f.run.halt,
                budget: f.run.budget,
                repeat: repeat_fact(f.repeat),
                limit: f.run.limit,
                taint: f.taint,
                run: policy_point::RunFact::NotAsked,
                mode: f.mode,
                rows: f.rows,
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
        // The pseudo-app of the external agents is the host's alone. The host's role makes one
        // other kind of call: the agent's own tool calls over its per-session edge, which are
        // exactly the actions an MCP client is offered (reach `Offered`, not host-only).
        if !may_make(&st.registry, who.role, &request.action, &decl) {
            return Err(early(
                decl.effect,
                CallRefusal::Denied(DenyCode::NotAllowed),
            ));
        }
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
        let action = request.action.clone();
        let end = |refusal: CallRefusal| {
            Box::new(Early {
                id,
                who: who.clone(),
                action: action.clone(),
                effect: decl.effect,
                space: space.clone(),
                refusal,
            })
        };
        admit(record).map_err(end)?;
        // An app's own Space is that app's alone, whatever the grants or the policy would say.
        if let SpaceAccess::Refused(_) =
            space_access(&SpaceScope::Only(space.clone()), &who.caller.app.name)
        {
            return Err(end(CallRefusal::Denied(DenyCode::NotAllowed)));
        }
        let turn_start = rule(who, &request, &record.turns);
        if turn_start == TurnStart::Refused {
            return Err(end(CallRefusal::Denied(DenyCode::NotAllowed)));
        }
        let named = request.handles();
        let target = resolve_target(record, request.target.clone()).map_err(|why| {
            end(CallRefusal::BadArgs {
                param: target_name(),
                why,
            })
        })?;
        let args = no_handle_names(&decl, &request.args)
            .and_then(|()| label_args(record, &who.voice, request.args.clone()))
            .and_then(|a| check_call(&decl, &request.action.app, &target, a))
            .map_err(|(param, why)| end(CallRefusal::BadArgs { param, why }))?;
        let request = CallRequest {
            args,
            target,
            ..request
        };
        let targets = entities(&request.target);
        // One writer per thing: a write to what another live job owns is refused, whatever the
        // policy, the grants and the permissions say.
        if decl.effect > Effect::Read && owned_by_another(st, record, &touched(&request)) {
            return Err(end(CallRefusal::Denied(DenyCode::OwnedByAnotherJob)));
        }
        let count = Count(u32::try_from(targets.len()).unwrap_or(u32::MAX));
        let taint = taint_fact(record.saw.untrusted, decl.effect);
        let consent = consent_for(
            grants,
            &who.grant_caller(),
            &decl,
            &request.action.app,
            &space,
            usage_of(&who.actor),
        );
        let consent = match turn_start {
            TurnStart::Preapproved => pass(consent),
            TurnStart::Unclaimed | TurnStart::Refused => consent,
        };
        let labels = arg_labels(&request.args, record);
        // The person set a helper's reach on its card: a call inside it asks no first-use
        // question. Outside it, and against a recorded denial, nothing changes.
        let consent = match &record.helper {
            Some(seat) => within_reach(
                consent,
                docket_core::covers_reach(&seat.reach, &decl, &request, &labels)
                    == docket_core::Coverage::Inside,
                taint,
            ),
            None => consent,
        };
        let cover = coverage(record.policy.as_ref(), &decl, &request, &labels, now);
        // The conversation's mode, read afresh for every call, so a switch applies from the next
        // step; the administrator's floor holds it, and locks rows to "asks".
        let (mode, rows) = self.mode_in_force(st, who.session.as_ref());
        let impact = impact_of(&decl, count, self.agent_config().mass_at);
        // Everything stateful is gathered here and ruled on by Cedar: the kill switch, the budgets,
        // the breaker, the limits (the person's, the assistant's and the floor's) and the taint.
        let caller = who.grant_caller();
        let limited = self.limits_on(&LimitCall {
            caller: &caller,
            app: &request.action.app,
            action: Some(&decl.name),
            space: &space,
            effect: decl.effect,
        });
        let goal = GoalKey {
            app: request.action.app.clone(),
            action: decl.name.clone(),
            kind: targets.first().map(|e| e.kind.clone()),
        };
        let digest = digest(&request.target, &request.args);
        let cost = Cost {
            effect: decl.effect,
            entities: count,
            depth,
            review: Reviewed::No,
        };
        let budget = &self.agent_config().budget;
        let run = run_facts(
            &st.kill,
            &space,
            &record.ledger,
            budget,
            &cost,
            now,
            &limited,
        );
        let repeat = repeated(&record.breaker, &goal, digest);
        let presets = self.presets();
        let ruled = presets
            .pdp()
            .decide_checked(&self.policy_request(&Facts {
                who,
                decl: &decl,
                request: &request,
                record,
                consent: &consent,
                coverage: CoverageState::from(&cover),
                count,
                impact,
                relation: relation_of(st, &space, &decl.on, &targets),
                run: &run,
                repeat,
                taint,
                mode,
                rows,
            }));
        crate::policylog::log_faults(&ruled, &record.task);
        let ruling = ruled.ruling;
        // A reviewed call also spends the reviews budget.
        let cost = Cost {
            review: match ruling {
                Ruling::AllowJudged(_) => Reviewed::Yes,
                _ => Reviewed::No,
            },
            ..cost
        };
        let charged = budget_fact(&record.ledger, budget, &cost, now);
        let gated = gate(&GateInputs {
            ruling: &ruling,
            impact,
            facts: &run,
            charged,
        });
        // A standing grant replaces only the ask: the refusals above have already run, and the
        // reviewers still look at what it lets through.
        let breaker = match admit(record) {
            Ok(()) => BreakerState::Running,
            Err(_) => BreakerState::Tripped,
        };
        let budget = match charged {
            BudgetFact::Within => BudgetState::Within,
            BudgetFact::Over(_) => BudgetState::Over,
        };
        let ctx = StandingCtx::new(caller, &decl, &request, breaker, budget);
        // A limit outranks every allowance: under one, no standing grant or approval stands in
        // for the question (the policy has already asked or refused for it).
        let limited = match limited {
            Limited::Free => crate::limits::rule_limit(&ruling),
            held => held,
        };
        let (pending, standing) = match limited {
            Limited::Free => ctx.lifted(
                &self.seams.grants().standing(),
                &record.approvals,
                &decl,
                impact,
                gated,
            ),
            limit => (gated, ctx.limited(limit)),
        };
        Ok(Prepared {
            id,
            who: who.clone(),
            space: space.clone(),
            review: Some(ReviewRequest {
                space,
                strictness: mode.strictness(),
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
            named,
            activation: None,
            classified,
            standing: Some(standing),
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
        let request_named = request.handles();
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
            digest: digest(&request.target, &request.args),
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
            named: request_named,
            activation: None,
            // The person's own calls are not gated, so they are not classified.
            classified: None,
            standing: None,
        }
    }
}

/// Whether a caller in `role` may make a call of `action`. The pseudo-app belongs to the host of
/// an external agent alone; that host makes it no call of another app but the ones the agent's
/// edge offers (`AgentReach::Offered` of an app that is not host-only), so a hostile agent cannot
/// reach an action nobody offered it by naming it.
fn may_make(
    registry: &crate::registry::Registry,
    role: CallerRole,
    action: &docket_core::ActionRef,
    decl: &ActionDecl,
) -> bool {
    // Giving a job to a helper and proposing one are the companion's planners' alone.
    if crate::companion::is_helper_action(action) {
        return role == CallerRole::Companion;
    }
    match (role == CallerRole::AcpAgent, is_agent_action(action)) {
        (_, true) => role == CallerRole::AcpAgent,
        (false, false) => true,
        (true, false) => {
            decl.reach == docket_core::AgentReach::Offered
                && registry
                    .get(&action.app)
                    .is_some_and(|m| m.manifest().visibility == docket_core::Visibility::Everyone)
        }
    }
}
