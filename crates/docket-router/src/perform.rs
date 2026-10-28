//! Driving one call through its lifecycle: the pure step decides, this carries out what it
//! asks (the reviewers, the app's dry run, the sheet, the app itself) and keeps the audit.

use crate::call::{CallEffect, CallEvent, CallState, call_step};
use crate::confirm::offers_grant;
use crate::deadline::within;
use crate::driven::{Driven, Next, Run, code_of, reviewer_model, unissued};
use crate::gate::Pending;
use crate::perform_sheet::asked_why;
use crate::prepared::Prepared;
use crate::router::Router;
use crate::seams::Seams;
use crate::watch::Watch;
use crate::who::Who;
use action_review::{DeniedBy, ReviewVerdict, Reviewer};
use docket_core::{
    ActivationToken, AuditRecord, CallEnd, CallRefusal, ConfirmAnswerKind, Confirmer, EventSink,
    Follow, Outcome, Undoable, halted,
};
use docket_permissions_core::{AlwaysOffer, ReviewError, ReviewMark, Stage};
use docket_vocab::{CallRequest, Clock, Depth, Millis, WindowKey};
use prov::SpaceScope;
use tracing::Instrument;

impl<S: Seams> Router<S> {
    /// One call, from arguments to answer.
    pub(crate) async fn perform_chain(
        &self,
        who: Who,
        request: CallRequest,
        window: Option<WindowKey>,
        activation: Option<ActivationToken>,
        watch: &Watch,
    ) -> Result<Outcome, CallRefusal> {
        let mut outcome = self
            .perform_once(&who, request, window.clone(), activation, Depth(0), watch)
            .await?;
        let mut depth = 0u8;
        while let Follow::Next(next) = outcome.follow.clone() {
            depth = depth.saturating_add(1);
            match self
                .perform_once(&who, next, window.clone(), None, Depth(depth), watch)
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
        activation: Option<ActivationToken>,
        depth: Depth,
        watch: &Watch,
    ) -> Result<Outcome, CallRefusal> {
        match self
            .prepare_classified(who, request, window.clone(), depth)
            .await
        {
            Err(early) => Err(self.finish_early(*early)),
            Ok(prepared) => {
                let prepared = self
                    .with_filled(Prepared {
                        activation,
                        ..prepared
                    })
                    .await;
                let driven = self
                    .drive(&prepared, watch)
                    .instrument(crate::spans::execute_tool(&prepared))
                    .await;
                self.note_delegation(&prepared, &driven);
                let changed = Self::changed_since_classified(&prepared, &driven);
                let first = self.finish(&prepared, driven).await;
                if !changed {
                    return first;
                }
                // The app's state moved between `Classify` and `Perform`: ask again at the
                // declared ceiling, without classifying afresh.
                match self.prepare_at_ceiling(&prepared, window, depth) {
                    Err(early) => Err(self.finish_early(*early)),
                    Ok(again) => {
                        let again = Prepared {
                            activation: prepared.activation.clone(),
                            filled: prepared.filled.clone(),
                            ..again
                        };
                        let driven = self
                            .drive(&again, watch)
                            .instrument(crate::spans::execute_tool(&again))
                            .await;
                        self.finish(&again, driven).await
                    }
                }
            }
        }
    }

    pub(crate) fn halt_scope(&self, space: &prov::SpaceId) -> Option<SpaceScope> {
        let st = self.locked();
        halted(&st.kill, space)?;
        Some(match st.kill.all {
            docket_core::Halt::Halted { .. } => SpaceScope::Any,
            docket_core::Halt::Running => SpaceScope::Only(space.clone()),
        })
    }

    async fn drive(&self, p: &Prepared, watch: &Watch) -> Driven {
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
                if let CallEffect::Progress(progress) = effect {
                    watch.tell(progress.clone()).await;
                }
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
            CallEffect::RecordGrant => match &p.standing {
                // An editor's or agent's "always" is the scoped grant the sheet offered, or
                // nothing: never the broad class grant.
                Some(s) if s.holds() => {
                    if let AlwaysOffer::Offered(scope) = s.offer(&p.decl, &asked_why(p)) {
                        self.record_standing(s.caller.clone(), scope);
                    }
                }
                _ => self.record_class_grant(p),
            },
            CallEffect::RecordNever => {
                run.denied_by = Some(DeniedBy::User);
                self.record_never(p);
            }
            CallEffect::RecordTerminalGrant => {
                let why = match &p.pending {
                    Pending::Confirm(why) => why.as_slice(),
                    _ => &[],
                };
                // Only a sheet that offered it may grant it.
                if offers_grant(p, why) {
                    self.record_terminal_grant(p);
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
        let limit = self.agent_config().review;
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
}
