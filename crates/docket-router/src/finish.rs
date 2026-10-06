//! What a call leaves behind when it ends: the one audit record, the breaker's tally, the
//! session's history and the task's ledger, and the outcome as its caller may read it.

use crate::driven::{Driven, decision_of, step_end};
use crate::handles::HandleValue;
use crate::labels::{Voice, absorb};
use crate::prepared::{Early, Prepared};
use crate::router::Router;
use crate::seams::{Clock, EventSink, Seams};
use crate::session::{SessionEffect, SessionEvent, session_step};
use crate::state::SessionRecord;
use crate::tasks::TaskState;
use action_review::note;
use docket_core::{
    AuditRecord, CallEnd, CallRefusal, DecidedBy, LedgerStep, Outcome, Preview, StepLine,
    StepShown, Undoable, Value,
};
use prov::{Integrity, Labelled, Source};

/// An outcome as the caller named by `voice` may read it: a planner gets untrusted text as a
/// handle the session holds, and never the app's preview, which may quote content.
fn present(record: &mut SessionRecord, voice: &Voice, mut outcome: Outcome) -> Outcome {
    if *voice != Voice::Model {
        return outcome;
    }
    outcome.show = Preview::None;
    outcome.value = outcome.value.map(|held| {
        let source = held
            .label
            .sources
            .iter()
            .next()
            .cloned()
            .unwrap_or(Source::Model(prov::ModelRole::Planner));
        match (&held.value, held.label.integrity) {
            (Value::Text(t) | Value::Url(t), Integrity::Untrusted) => {
                let handle = record.handles.mint(
                    Labelled {
                        value: HandleValue::Text(t.clone()),
                        label: held.label.clone(),
                    },
                    source,
                );
                Labelled {
                    value: Value::Handle(handle),
                    label: held.label,
                }
            }
            (Value::Entity(e), _) => {
                record.known.insert(e.clone());
                held
            }
            (Value::Entities(es), _) => {
                record.known.extend(es.iter().cloned());
                held
            }
            _ => held,
        }
    });
    outcome
}

impl<S: Seams> Router<S> {
    /// Ends a call that went through the gate: audit it, tally it, remember it.
    pub(crate) fn finish(&self, p: &Prepared, driven: Driven) -> Result<Outcome, CallRefusal> {
        let at = self.seams.clock().now();
        self.seams.sink().append(AuditRecord::Call {
            at,
            call: p.id,
            actor: p.who.actor.clone(),
            action: p.request.action.clone(),
            targets: p.targets.clone(),
            effect: p.decl.effect,
            space: p.space.clone(),
            decided: driven.decided.clone(),
            end: driven.end.clone(),
        });
        let mark = decision_of(&driven, p, at);
        let mut presented = driven.outcome.clone();
        if let Some(session) = &p.who.session {
            let mut st = self.locked();
            let breaker = self.agent_config().breaker;
            let mut tripped = None;
            if let Some(record) = st.sessions.get_mut(session) {
                if let Some(mark) = mark {
                    let (next, trip) = note(std::mem::take(&mut record.breaker), mark, &breaker);
                    record.breaker = next;
                    if let Some(trip) = trip {
                        let (state, effects) =
                            session_step(record.state, SessionEvent::BreakerTrip(trip));
                        record.state = state;
                        tripped = effects.into_iter().find_map(|e| match e {
                            SessionEffect::EmitBreakerTripped(t) => Some(t),
                            _ => None,
                        });
                    }
                }
                if let Some(value) = presented.as_ref().and_then(|o| o.value.as_ref()) {
                    absorb(record, &value.label);
                }
                let end = step_end(&driven, presented.as_ref());
                record.history.push(StepLine {
                    call: p.id,
                    action: p.request.action.clone(),
                    effect: p.decl.effect,
                    end,
                    shown: StepShown::Full,
                });
                presented = presented.map(|o| present(record, &p.who.voice, o));
                let line = record.history.last().cloned();
                let task = record.task.clone();
                if let Some(t) = st.tasks.get_mut(&task) {
                    t.ledger.steps.push(LedgerStep {
                        call: p.id,
                        action: p.decl.name.clone(),
                        targets: p.targets.clone(),
                        effect: p.decl.effect,
                        end: driven.end.clone(),
                        undo: driven.undo,
                    });
                    t.last = line;
                    if tripped.is_some() {
                        t.state = TaskState::NeedsYou;
                    }
                }
            }
            drop(st);
            if let Some(trip) = tripped {
                self.seams.sink().append(AuditRecord::Breaker {
                    at,
                    session: session.clone(),
                    trip,
                });
            }
        }
        match driven.end {
            CallEnd::Done => presented
                .map(|mut outcome| {
                    // The app's token stays the journal's: the caller is given the row.
                    if let Some(row) = driven.undo {
                        outcome.undo = Undoable::Journaled(row);
                    }
                    outcome
                })
                .ok_or(CallRefusal::Timeout),
            CallEnd::Refused(why) => Err(why),
        }
    }

    /// Ends a call that never reached the gate.
    pub(crate) fn finish_early(&self, early: Early) -> CallRefusal {
        self.seams.sink().append(AuditRecord::Call {
            at: self.seams.clock().now(),
            call: early.id,
            actor: early.who.actor.clone(),
            action: early.action,
            targets: vec![],
            effect: early.effect,
            space: early.space,
            decided: DecidedBy::Policy(vec![]),
            end: CallEnd::Refused(early.refusal.clone()),
        });
        early.refusal
    }
}
