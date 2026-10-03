//! Driving one call through its lifecycle: the pure step decides, this carries out what it
//! asks (the reviewers, the app's dry run, the sheet, the app itself) and keeps the audit.

use crate::call::{CallEffect, CallEvent, CallState, call_step};
use crate::confirm::confirm_request;
use crate::deadline::within;
use crate::driven::{Driven, Next, Run, code_of, reviewer_model, unissued};
use crate::gate::Pending;
use crate::prepared::{Prepared, grants_for};
use crate::router::Router;
use crate::seams::{AppFault, AppLink, Clock, EventSink, GrantStore, Seams};
use crate::who::Who;
use action_review::{DeniedBy, ReviewVerdict, Reviewer};
use docket_core::{
    AppRefusal, AskReason, AuditRecord, CallEnd, CallRefusal, CallRequest, ConfirmAnswer,
    ConfirmAnswerKind, ConfirmEnd, ConfirmId, ConfirmRequest, Confirmer, Depth, Follow, Invocation,
    Millis, Outcome, PolicyId, ReviewError, ReviewMark, Reviewed, Stage, UndoId, Undoable,
    WindowKey, charge, halted,
};
use porter_core::GrantId;
use prov::{Actor, AgentRole, SpaceScope};

impl<S: Seams> Router<S> {
    /// One call, from arguments to answer.
    pub(crate) async fn perform_chain(
        &self,
        who: Who,
        request: CallRequest,
        window: Option<WindowKey>,
    ) -> Result<Outcome, CallRefusal> {
        let mut outcome = self
            .perform_once(&who, request, window.clone(), Depth(0))
            .await?;
        let mut depth = 0u8;
        while let Follow::Next(next) = outcome.follow.clone() {
            depth = depth.saturating_add(1);
            match self
                .perform_once(&who, next, window.clone(), Depth(depth))
                .await
            {
                Ok(done) => outcome = done,
                Err(_) => {
                    outcome.follow = Follow::Nothing;
                    break;
                }
            }
        }
        Ok(outcome)
    }

    async fn perform_once(
        &self,
        who: &Who,
        request: CallRequest,
        window: Option<WindowKey>,
        depth: Depth,
    ) -> Result<Outcome, CallRefusal> {
        match self.prepare(who, request, window, depth) {
            Err(early) => Err(self.finish_early(*early)),
            Ok(prepared) => {
                let driven = self.drive(&prepared).await;
                self.finish(&prepared, driven)
            }
        }
    }

    fn halt_scope(&self, space: &prov::SpaceId) -> Option<SpaceScope> {
        let st = self.locked();
        halted(&st.kill, space)?;
        Some(match st.kill.all {
            docket_core::Halt::Halted { .. } => SpaceScope::Any,
            docket_core::Halt::Running => SpaceScope::Only(space.clone()),
        })
    }

    async fn drive(&self, p: &Prepared) -> Driven {
        let mut run = Run::new();
        run.state = call_step(run.state.clone(), CallEvent::ArgsChecked(Ok(()))).0;
        if matches!(p.pending, Pending::Refuse(_)) {
            run.denied_by = Some(DeniedBy::Policy);
        }
        let mut event = CallEvent::Gated(p.pending.clone());
        loop {
            let (state, effects) = call_step(run.state.clone(), event);
            run.state = state;
            for effect in &effects {
                self.carry_out(p, &mut run, effect);
            }
            if matches!(run.state, CallState::Done(_)) {
                break;
            }
            event = match self.next_event(p, &mut run, &effects).await {
                Next::Event(e) => e,
                Next::Stop => break,
            };
        }
        let decided = run.decided_by(p);
        let end = match run.state {
            CallState::Done(CallEnd::Refused(CallRefusal::Halted(SpaceScope::Any))) => {
                CallEnd::Refused(CallRefusal::Halted(
                    self.halt_scope(&p.space).unwrap_or(SpaceScope::Any),
                ))
            }
            CallState::Done(end) => end,
            _ => CallEnd::Refused(CallRefusal::Timeout),
        };
        Driven {
            end,
            outcome: run.outcome,
            decided,
            denied_by: run.denied_by,
            undo: run.undo,
        }
    }

    /// The effects that need no await: the consent store, the breaker's tally, the journal.
    fn carry_out(&self, p: &Prepared, run: &mut Run, effect: &CallEffect) {
        match effect {
            CallEffect::RecordGrant => {
                let now = self.seams.clock().now();
                let id = {
                    let n = self.locked().mint();
                    GrantId::parse(&format!("g-{n}")).ok()
                };
                if let Some(id) = id {
                    for grant in grants_for(p, id, now, p.who.grant_caller()) {
                        self.seams.grants().record(grant);
                    }
                }
            }
            CallEffect::NoteDenial => {
                run.denied_by
                    .get_or_insert(if run.receipt.is_none() && !run.verdicts.is_empty() {
                        DeniedBy::Reviewer
                    } else {
                        DeniedBy::Policy
                    });
            }
            CallEffect::Unconfirmed(end) => {
                run.denied_by = Some(DeniedBy::User);
                self.seams.sink().append(AuditRecord::Confirm {
                    at: self.seams.clock().now(),
                    id: run.confirm.clone().unwrap_or_else(unissued),
                    answer: ConfirmAnswerKind::Ended(*end),
                    input: None,
                });
            }
            CallEffect::Journal => {
                if let Some(outcome) = &run.outcome
                    && let Undoable::Yes(token) = &outcome.undo
                {
                    run.undo = Some(self.journal_undo(p, outcome, token));
                }
            }
            CallEffect::Audit(_)
            | CallEffect::Progress(_)
            | CallEffect::StartReview(_)
            | CallEffect::DryRun
            | CallEffect::Confirm(_)
            | CallEffect::CancelConfirm
            | CallEffect::Dispatch => {}
        }
    }

    fn journal_undo(
        &self,
        p: &Prepared,
        outcome: &Outcome,
        token: &docket_core::UndoToken,
    ) -> UndoId {
        let now = self.seams.clock().now();
        let said = outcome.said.clone().unwrap_or_else(|| p.decl.label.clone());
        let (run_id, session) = match (&p.who.actor, &p.who.session) {
            (
                Actor::Companion {
                    role: AgentRole::Cua { run },
                    ..
                },
                session,
            ) => (Some(run.clone()), session.clone()),
            (_, session) => (None, session.clone()),
        };
        self.locked().journal.record(
            now,
            p.who.actor.clone(),
            p.request.action.clone(),
            said,
            token.clone(),
            run_id,
            session,
        )
    }

    async fn next_event(&self, p: &Prepared, run: &mut Run, effects: &[CallEffect]) -> Next {
        let dispatching = effects.contains(&CallEffect::Dispatch);
        if self.halt_scope(&p.space).is_some()
            && (dispatching || !matches!(run.state, CallState::Dispatched))
        {
            if dispatching {
                run.state = CallState::Done(CallEnd::Refused(CallRefusal::Halted(SpaceScope::Any)));
                return Next::Stop;
            }
            if let CallState::Confirming(id) = &run.state {
                self.seams.confirmer().cancel(id).await;
            }
            return Next::Event(CallEvent::Halted);
        }
        for effect in effects {
            match effect {
                CallEffect::StartReview(stage) => return self.review(p, run, *stage).await,
                CallEffect::DryRun => return self.dry_run(p, run).await,
                CallEffect::Confirm(request) => return self.confirm(p, run, request).await,
                CallEffect::Dispatch => return self.dispatch(p, run).await,
                _ => {}
            }
        }
        Next::Stop
    }

    async fn review(&self, p: &Prepared, run: &mut Run, stage: Stage) -> Next {
        let Some(request) = &p.review else {
            return Next::Event(CallEvent::Verdict(stage, Err(ReviewError::Unavailable)));
        };
        let began = self.seams.clock().now();
        let limit = self.config.review;
        let wait = match stage {
            Stage::Quick => limit.quick,
            Stage::Deliberate => limit.deliberate,
            Stage::SecondOpinion => limit.second,
        };
        let verdict = within(
            self.seams.clock().after(wait),
            self.seams.reviewer().review(stage, request),
        )
        .await
        .unwrap_or(Err(ReviewError::Timeout));
        let took = self.seams.clock().now().0.saturating_sub(began.0);
        let code = code_of(&verdict);
        self.seams.sink().append(AuditRecord::Review {
            at: self.seams.clock().now(),
            call: p.id,
            mark: ReviewMark {
                stage,
                verdict: verdict.as_ref().map(ReviewVerdict::kind).map_err(|e| *e),
                code,
                latency: Millis(u32::try_from(took.saturating_mul(1000)).unwrap_or(u32::MAX)),
                model: reviewer_model(stage),
            },
        });
        run.codes.push((stage, code));
        run.verdicts.push((stage, verdict.clone()));
        Next::Event(CallEvent::Verdict(stage, verdict))
    }

    async fn dry_run(&self, p: &Prepared, run: &mut Run) -> Next {
        let answer = match p.decl.dry_run {
            docket_core::DryRun::None => Err(AppRefusal::Unsupported),
            docket_core::DryRun::Preview => {
                self.seams
                    .link()
                    .dry_run(&p.request.action.app, self.invocation(p))
                    .await
            }
        };
        run.preview = answer.as_ref().ok().cloned();
        match self.sheet_for(p, run) {
            Some(request) => Next::Event(CallEvent::Previewed(Box::new(request))),
            None => Next::Event(CallEvent::Answered(ConfirmAnswer::Ended(
                ConfirmEnd::Cancelled,
            ))),
        }
    }

    fn invocation(&self, p: &Prepared) -> Invocation {
        Invocation {
            call: p.id,
            action: p.decl.name.clone(),
            target: p.request.target.clone(),
            args: p.request.args.clone(),
            actor: p.who.actor.clone(),
            origin: p.request.origin,
            space: p.space.clone(),
        }
    }

    /// The sheet for a call that goes to the person: its reasons, the preview the app gave (or
    /// the plain argument lines), and a freshly minted id.
    fn sheet_for(&self, p: &Prepared, run: &Run) -> Option<ConfirmRequest> {
        let mut st = self.locked();
        let n = st.mint();
        let id = ConfirmId::parse(&format!("c-{n}")).ok()?;
        let record = p.who.session.as_ref().and_then(|s| st.sessions.get(s));
        let reasons = match &p.pending {
            crate::gate::Pending::Confirm(r) => r.clone(),
            _ => vec![AskReason::Rule(PolicyId("review".into()))],
        };
        Some(confirm_request(
            id,
            p,
            record,
            &reasons,
            run.preview.as_ref(),
            self.config.confirm_expiry,
        ))
    }

    async fn confirm(&self, p: &Prepared, run: &mut Run, request: &ConfirmRequest) -> Next {
        let id = request.id.clone();
        self.locked().pending.insert(id.clone(), p.space.clone());
        run.confirm = Some(id.clone());
        let answer = self.seams.confirmer().confirm(request.clone()).await;
        self.locked().pending.remove(&id);
        if self.halt_scope(&p.space).is_some() {
            self.seams.confirmer().cancel(&id).await;
            return Next::Event(CallEvent::Halted);
        }
        if let ConfirmAnswer::Allowed { scope, receipt } = &answer {
            run.receipt = Some(receipt.clone());
            self.seams.sink().append(AuditRecord::Confirm {
                at: self.seams.clock().now(),
                id: id.clone(),
                answer: ConfirmAnswerKind::Allowed(*scope),
                input: Some(receipt.input),
            });
        }
        Next::Event(CallEvent::Answered(answer))
    }

    async fn dispatch(&self, p: &Prepared, run: &mut Run) -> Next {
        let reviewed = if run.verdicts.is_empty() {
            Reviewed::No
        } else {
            Reviewed::Yes
        };
        if let Some(session) = &p.who.session {
            let now = self.seams.clock().now();
            let mut st = self.locked();
            if let Some(record) = st.sessions.get_mut(session) {
                let cost = docket_core::Cost {
                    review: reviewed,
                    ..p.cost
                };
                match charge(&record.ledger, &self.config.budget, &cost, now) {
                    Ok(ledger) => record.ledger = ledger,
                    Err(kind) => {
                        run.state =
                            CallState::Done(CallEnd::Refused(CallRefusal::OverBudget(kind)));
                        return Next::Stop;
                    }
                }
            }
        }
        let answer = self
            .seams
            .link()
            .perform(&p.request.action.app, self.invocation(p), p.decl.latency)
            .await;
        run.outcome = answer.as_ref().ok().cloned();
        match answer {
            Err(AppFault::TimedOut) => Next::Event(CallEvent::AppTimedOut),
            Ok(outcome) => Next::Event(CallEvent::AppAnswered(Box::new(Ok(outcome)))),
            Err(AppFault::Refused(why)) => Next::Event(CallEvent::AppAnswered(Box::new(Err(why)))),
        }
    }
}
