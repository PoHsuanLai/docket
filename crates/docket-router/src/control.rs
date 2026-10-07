//! The halt and the journal: stopping everything, resuming, and undoing what agents did. The
//! app's own stack stays the truth for Cmd+Z; the journal follows it and can undo a task.

use crate::messaging::age;
use crate::router::Router;
use crate::seams::{AppLink, Clock, EventSink, Seams};
use crate::session::SessionEvent;
use docket_core::{
    AuditRecord, CallerId, CallerRole, Confirmer, Halt, HaltCause, IntentsReply, JournalFilter,
    UndoEntry, UndoFault, UndoId, UndoReport, UndoScope, UndoState,
};
use porter_core::Count;
use prov::{Actor, SpaceScope};

fn actor_of(caller: &CallerId, role: CallerRole) -> Actor {
    match role {
        CallerRole::App => Actor::App {
            app: caller.app.name.clone(),
        },
        CallerRole::Cli => Actor::Cli,
        _ => Actor::User {
            via: caller.app.name.clone(),
        },
    }
}

impl<S: Seams> Router<S> {
    /// `.Control.Halt`: everything in scope stops. A confirmation on a sheet is withdrawn and a
    /// session in a halted Space closes; an app call already in flight cannot be recalled.
    pub(crate) async fn control_halt(&self, scope: SpaceScope, cause: HaltCause) -> IntentsReply {
        let now = self.seams.clock().now();
        let withdrawn = {
            let mut st = self.locked();
            let halt = Halt::Halted {
                since: now,
                by: cause,
            };
            match &scope {
                SpaceScope::Any => st.kill.all = halt,
                SpaceScope::Only(space) => {
                    st.kill.spaces.insert(space.clone(), halt);
                }
            }
            for record in st.sessions.values_mut() {
                let inside = match &scope {
                    SpaceScope::Any => true,
                    SpaceScope::Only(space) => *space == record.space,
                };
                if inside {
                    record.apply(SessionEvent::SpaceHalted);
                }
            }
            st.pending
                .iter()
                .filter(|(_, space)| match &scope {
                    SpaceScope::Any => true,
                    SpaceScope::Only(only) => only == *space,
                })
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };
        self.seams.sink().append(AuditRecord::Halt {
            at: now,
            scope,
            cause,
        });
        for id in withdrawn {
            self.seams.confirmer().cancel(&id).await;
        }
        IntentsReply::Done
    }

    /// `.Control.Resume`: the only way back to running. It also reopens the terminal sessions the
    /// breaker paused (`resume_terminals`): the person at the control centre is the one who
    /// can say the terminal may go on.
    pub(crate) fn control_resume(&self, scope: SpaceScope) -> IntentsReply {
        let now = self.seams.clock().now();
        self.resume_terminals(&scope);
        {
            let mut st = self.locked();
            match &scope {
                SpaceScope::Any => {
                    st.kill.all = Halt::Running;
                    st.kill.spaces.clear();
                }
                SpaceScope::Only(space) => {
                    st.kill.spaces.remove(space);
                    // A global halt overrides every Space, so resuming one lifts it for that Space
                    // alone: the global halt becomes a halt of each other Space the router knows.
                    if let Halt::Halted { .. } = st.kill.all {
                        let halt = std::mem::replace(&mut st.kill.all, Halt::Running);
                        let others: Vec<_> = st
                            .sessions
                            .values()
                            .map(|r| r.space.clone())
                            .filter(|other| other != space)
                            .collect();
                        for other in others {
                            st.kill.spaces.entry(other).or_insert(halt);
                        }
                    }
                }
            }
        }
        self.seams.sink().append(AuditRecord::Halt {
            at: now,
            scope,
            cause: HaltCause::ControlCentre,
        });
        IntentsReply::Done
    }

    /// `.Control.Journal`, newest first. A terminal reads only the rows of its own acts.
    pub(crate) fn control_journal(&self, role: CallerRole, filter: &JournalFilter) -> IntentsReply {
        let st = self.locked();
        let rows: Vec<UndoEntry> = st
            .journal
            .entries()
            .iter()
            .rev()
            .filter(|e| role != CallerRole::Cli || e.actor == Actor::Cli)
            .filter(|e| {
                filter
                    .run
                    .as_ref()
                    .is_none_or(|r| e.run.as_ref() == Some(r))
            })
            .filter(|e| {
                filter
                    .session
                    .as_ref()
                    .is_none_or(|s| e.session.as_ref() == Some(s))
            })
            .take(filter.limit.0 as usize)
            .cloned()
            .collect();
        IntentsReply::Journal(rows)
    }

    /// The app's own stack undid something (Cmd+Z): the journal follows.
    pub fn note_app_undo(&self, id: UndoId, by: Actor) {
        let at = self.seams.clock().now();
        let marked = self
            .locked()
            .journal
            .mark(id, UndoState::Undone { by: by.clone() });
        if marked {
            self.seams.sink().append(AuditRecord::Undo {
                at,
                entry: id,
                by: by.clone(),
                end: UndoState::Undone { by },
            });
        }
    }

    async fn undo_one(&self, id: UndoId, by: &Actor) -> Result<(), UndoFault> {
        let entry = {
            let mut st = self.locked();
            let entry = st.journal.get(id).cloned().ok_or(UndoFault::Gone)?;
            if entry.state != UndoState::Available {
                return Err(UndoFault::Gone);
            }
            st.journal.mark(id, UndoState::Undoing);
            entry
        };
        let result = self
            .seams
            .link()
            .undo(&entry.action.app, &entry.token, by)
            .await;
        let end = match &result {
            Ok(()) => UndoState::Undone { by: by.clone() },
            Err(fault) => UndoState::Failed(*fault),
        };
        self.locked().journal.mark(id, end.clone());
        self.seams.sink().append(AuditRecord::Undo {
            at: self.seams.clock().now(),
            entry: id,
            by: by.clone(),
            end,
        });
        result
    }

    /// `.Run.Undo`.
    pub(crate) async fn run_undo(
        &self,
        caller: &CallerId,
        role: CallerRole,
        id: UndoId,
    ) -> IntentsReply {
        let own = self
            .locked()
            .journal
            .get(id)
            .is_none_or(|e| role != CallerRole::Cli || e.actor == Actor::Cli);
        if !own {
            return IntentsReply::Refused(docket_core::WireRefusal::NotAllowed);
        }
        IntentsReply::Undone(self.undo_one(id, &actor_of(caller, role)).await)
    }

    /// `.Run.UndoAll`: newest first, stopping at the first that cannot be undone. No locks: the
    /// person may have changed the thing since, and that is reported, not prevented.
    pub(crate) async fn run_undo_all(
        &self,
        caller: &CallerId,
        role: CallerRole,
        scope: UndoScope,
    ) -> IntentsReply {
        let by = actor_of(caller, role);
        let plan = {
            let st = self.locked();
            let sessions: Vec<_> = match &scope {
                UndoScope::Task(task) => {
                    let mut own: Vec<_> = st
                        .sessions
                        .iter()
                        .filter(|(_, r)| &r.task == task)
                        .map(|(id, _)| id.clone())
                        .collect();
                    own.sort_by_key(age);
                    own
                }
                UndoScope::Entry(_) | UndoScope::Run(_) => vec![],
            };
            st.journal.plan_undo(&scope, &sessions)
        };
        let mut undone = 0u32;
        let mut stopped = None;
        for id in plan {
            match self.undo_one(id, &by).await {
                Ok(()) => undone += 1,
                Err(fault) => {
                    stopped = Some(fault);
                    break;
                }
            }
        }
        IntentsReply::UndoneAll(UndoReport {
            undone: Count(undone),
            stopped,
        })
    }
}
