//! What a call leaves behind when it ends: the one audit record, the breaker's tally, the
//! session's history and the task's ledger, and the outcome as its caller may read it.

use crate::driven::{Driven, decision_of, step_end};
use crate::handles::HandleValue;
use crate::labels::{Voice, absorb};
use crate::prepared::{Early, Prepared};
use crate::router::Router;
use crate::seams::{Clock, EventSink, Seams};
use crate::session::{SessionEffect, SessionEvent};
use crate::state::SessionRecord;
use crate::tasks::TaskState;
use action_review::note;
use docket_core::{
    AuditRecord, CallEnd, CallRefusal, DecidedBy, LedgerStep, Outcome, Preview, StepEnd, StepLine,
    StepShown, Undoable, Value,
};
use docket_session::{BreakerNote, SessionEntry};
use prov::{Integrity, Labelled, Source};

/// An outcome as the caller named by `voice` may read it: a planner gets untrusted text, and
/// every thing an app returns, as a handle the session holds, and never the app's preview,
/// which may quote content. Things are handles because a planner names them by `{"handle": n}`.
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
                let handle = record.handles.mint_entity(e.clone(), held.label.clone());
                Labelled {
                    value: Value::Handle(handle),
                    label: held.label,
                }
            }
            (Value::Entities(es), _) if !es.is_empty() => {
                record.known.extend(es.iter().cloned());
                let handles = es
                    .iter()
                    .map(|e| {
                        Value::Handle(record.handles.mint_entity(e.clone(), held.label.clone()))
                    })
                    .collect();
                Labelled {
                    value: Value::List(handles),
                    label: held.label,
                }
            }
            _ => held,
        }
    });
    outcome
}

impl<S: Seams> Router<S> {
    /// Ends a call that went through the gate: audit it, tally it, remember it.
    pub(crate) async fn finish(
        &self,
        p: &Prepared,
        driven: Driven,
    ) -> Result<Outcome, CallRefusal> {
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
        // Write-ahead: an untrusted value is not revealed, or held as a handle, until the
        // session's taint is on the record. If it cannot be, the call ran but its result is
        // withheld and its step ends interrupted.
        let withheld = self.taint_ahead_of(p, &driven).await;
        let mut presented = match withheld {
            Some(_) => None,
            None => driven.outcome.clone(),
        };
        if let Some(session) = &p.who.session {
            let mut st = self.locked();
            let breaker = self.agent_config().breaker;
            let mut tripped = None;
            if let Some(record) = st.sessions.get_mut(session) {
                if let Some(mark) = mark {
                    let (next, trip) = note(std::mem::take(&mut record.breaker), mark, &breaker);
                    record.breaker = next;
                    if let Some(trip) = trip {
                        let effects = record.apply(SessionEvent::BreakerTrip(trip));
                        record
                            .wal
                            .note(SessionEntry::Breaker(BreakerNote::Tripped(trip)));
                        tripped = effects.into_iter().find_map(|e| match e {
                            SessionEffect::EmitBreakerTripped(t) => Some(t),
                            _ => None,
                        });
                    }
                }
                if let Some(value) = presented.as_ref().and_then(|o| o.value.as_ref()) {
                    absorb(record, &value.label);
                }
                let end = match withheld {
                    Some(_) => StepEnd::Interrupted,
                    None => step_end(&driven, presented.as_ref()),
                };
                record.history.push(StepLine {
                    call: p.id,
                    action: p.request.action.clone(),
                    effect: p.decl.effect,
                    end,
                    shown: StepShown::Full,
                    with: p.named.clone(),
                });
                presented = presented.map(|o| present(record, &p.who.voice, o));
                // The handles this call minted go on the record before the step that names them.
                record.gather();
                let line = record.history.last().cloned();
                if let Some(line) = &line {
                    record.wal.note(SessionEntry::Step(line.clone()));
                }
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
        if let Some(refusal) = withheld {
            return Err(refusal);
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

    /// The write-ahead step of a call's end: `Some` when the call's value is untrusted and the
    /// session's taint could not be put on the record, so the value must not be revealed.
    async fn taint_ahead_of(&self, p: &Prepared, driven: &Driven) -> Option<CallRefusal> {
        let session = p.who.session.as_ref()?;
        let untrusted = driven
            .outcome
            .as_ref()
            .and_then(|o| o.value.as_ref())
            .is_some_and(|v| v.label.integrity == Integrity::Untrusted);
        if !untrusted {
            return None;
        }
        self.ahead_of_reveal(session, Some(p.id)).await.err()
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
