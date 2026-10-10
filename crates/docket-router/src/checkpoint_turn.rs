//! The restore point of a turn: saved when the turn is recorded, before the host is told, and
//! pruned afterwards. A store that fails, or takes longer than the configured wait, costs the
//! turn nothing but its point: the log says `Skipped(Failed)` and the turn goes on.

use crate::deadline::within;
use crate::router::Router;
use crate::seams::{Clock, Seams};
use docket_checkpoint::{
    CheckpointStore, Decision, DropAsk, Saved, StoreFault, TakeAsk, WorkRoot, decide, next_id,
    retention,
};
use docket_core::{
    AgentConfig, CheckpointEvent, CheckpointId, CheckpointNote, Millis, Retention, Rewind,
    SkipReason, TurnId,
};
use docket_session::{Logged, Read, SessionEntry, SessionLog, resume_plan};
use porter_core::Count;
use prov::SessionId;
use std::future::Future;

/// Marks a session's workspace as being saved or restored; leaving the scope (or dropping the
/// future that holds it) clears the mark.
pub(crate) struct Stepping<'a, S: Seams> {
    router: &'a Router<S>,
    session: SessionId,
}

impl<S: Seams> Drop for Stepping<'_, S> {
    fn drop(&mut self) {
        self.router.locked().stepping.remove(&self.session);
    }
}

/// The restore-point notes of a session's log, oldest first.
pub(crate) fn notes_of(rows: &[Logged]) -> Vec<CheckpointNote> {
    rows.iter()
        .filter_map(|row| match &row.read {
            Read::Entry(entry) => match entry.as_ref() {
                SessionEntry::Checkpoint(note) => Some(note.clone()),
                _ => None,
            },
            Read::Legacy(_) | Read::Unreadable(_) => None,
        })
        .collect()
}

/// Why a store could not save, as the list tells it.
fn skip_of(fault: StoreFault) -> SkipReason {
    match fault {
        StoreFault::NoHistory => SkipReason::NoHistory,
        StoreFault::TooLarge => SkipReason::TooLarge,
        _ => SkipReason::Failed,
    }
}

impl<S: Seams> Router<S> {
    /// Takes the mark of `session`, unless a save or a restore already holds it.
    pub(crate) fn begin_step(&self, session: &SessionId) -> Option<Stepping<'_, S>> {
        self.locked()
            .stepping
            .insert(session.clone())
            .then(|| Stepping {
                router: self,
                session: session.clone(),
            })
    }

    /// `work`, or `None` if it takes longer than the configured wait.
    pub(crate) async fn bounded<T>(
        &self,
        cfg: &AgentConfig,
        work: impl Future<Output = T>,
    ) -> Option<T> {
        let wait = Millis(cfg.checkpoint_wait.0.saturating_mul(1000));
        within(self.seams.clock().after(wait), work).await
    }

    /// The number for the next point of `session`: above every one the log names and every one
    /// the store holds, so a number is never used twice.
    pub(crate) fn next_point(&self, session: &SessionId, held: &[Saved]) -> CheckpointId {
        let named = self
            .locked()
            .sessions
            .get(session)
            .map_or(CheckpointId(1), |record| next_id(&record.checkpoints));
        let held_top = held
            .iter()
            .map(|saved| saved.id.0)
            .max()
            .map_or(1, |top| top.saturating_add(1));
        CheckpointId(named.0.max(held_top))
    }

    /// Queues the note of `event` on the session's record, and keeps it for the list.
    pub(crate) fn note_checkpoint(
        &self,
        session: &SessionId,
        turn: TurnId,
        event: CheckpointEvent,
    ) {
        let note = CheckpointNote {
            turn,
            at: self.seams.clock().now(),
            event,
        };
        if let Some(record) = self.locked().sessions.get_mut(session) {
            record.wal.note(SessionEntry::Checkpoint(note.clone()));
            record.checkpoints.push(note);
        }
    }

    /// What a turn does about a restore point, once it is recorded. A session with no workspace
    /// writes nothing at all.
    pub(crate) async fn checkpoint_step(&self, session: &SessionId, turn: TurnId) {
        let (rewind, cwd) = {
            let st = self.locked();
            let Some(record) = st.sessions.get(session) else {
                return;
            };
            (
                record
                    .external
                    .as_ref()
                    .map_or(Rewind::Docket, |agent| agent.rewind),
                record.cwd.clone(),
            )
        };
        let cfg = self.agent_config();
        match decide(rewind, cwd.as_ref()) {
            Decision::Take(root) => {
                let Some(_step) = self.begin_step(session) else {
                    let failed = CheckpointEvent::Skipped(SkipReason::Failed);
                    return self.note_checkpoint(session, turn, failed);
                };
                let event = self.take_point(session, &root, &cfg).await;
                let saved = matches!(event, CheckpointEvent::Taken(_));
                self.note_checkpoint(session, turn, event);
                if saved {
                    self.prune_session(session, &root, &cfg).await;
                }
            }
            Decision::Skip(why) => {
                self.note_checkpoint(session, turn, CheckpointEvent::Skipped(why));
            }
            _ => {}
        }
    }

    async fn take_point(
        &self,
        session: &SessionId,
        root: &WorkRoot,
        cfg: &AgentConfig,
    ) -> CheckpointEvent {
        match self
            .bounded(cfg, self.take_numbered(session, root, cfg))
            .await
        {
            Some(Ok(saved)) => CheckpointEvent::Taken(saved.id),
            Some(Err(fault)) => CheckpointEvent::Skipped(skip_of(fault)),
            None => CheckpointEvent::Skipped(SkipReason::Failed),
        }
    }

    async fn take_numbered(
        &self,
        session: &SessionId,
        root: &WorkRoot,
        cfg: &AgentConfig,
    ) -> Result<Saved, StoreFault> {
        let store = self.seams.checkpoints();
        let held = store.held(root, session).await?;
        store
            .take(TakeAsk {
                root: root.clone(),
                session: session.clone(),
                id: self.next_point(session, &held),
                at: self.seams.clock().now(),
                max: cfg.checkpoint_max_files,
            })
            .await
    }

    /// Forgets the points of `session` that the retention rule drops. The notes stay in the log;
    /// the list shows those points as cleared.
    pub(crate) async fn prune_session(
        &self,
        session: &SessionId,
        root: &WorkRoot,
        cfg: &AgentConfig,
    ) -> Count {
        let keep = Retention {
            last: cfg.checkpoint_keep,
            days: cfg.checkpoint_days,
        };
        let now = self.seams.clock().now();
        let store = self.seams.checkpoints();
        let work = async {
            let held = store.held(root, session).await.ok()?;
            let ids = retention(&held, now, keep);
            if ids.is_empty() {
                return None;
            }
            let ask = DropAsk {
                root: root.clone(),
                session: session.clone(),
                ids,
            };
            store.drop_points(ask).await.ok()
        };
        self.bounded(cfg, work).await.flatten().unwrap_or(Count(0))
    }

    /// Prunes every stored session's points: the daemon runs it at start and once a day. A
    /// session's workspace is read from its log, so a closed session is pruned too. The number
    /// of points forgotten.
    pub async fn prune_checkpoints(&self) -> Count {
        let cfg = self.agent_config();
        let Ok(all) = self.seams.log().sessions().await else {
            return Count(0);
        };
        let mut forgotten = 0u32;
        for session in all {
            let Ok(rows) = self.rows_of(&session).await else {
                continue;
            };
            let root = resume_plan(&rows)
                .ok()
                .and_then(|plan| plan.opening.cwd)
                .and_then(|cwd| WorkRoot::of(&cwd).ok());
            if let Some(root) = root {
                let dropped = self.prune_session(&session, &root, &cfg).await;
                forgotten = forgotten.saturating_add(dropped.0);
            }
        }
        Count(forgotten)
    }
}
