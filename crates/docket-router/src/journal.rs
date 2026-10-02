//! The router's undo journal: who did what, newest last, and what "undo all" would undo. The
//! app's own stack stays the truth for Cmd+Z; the journal follows it through `UndoChanged`.

use docket_core::{
    ActionRef, LabelText, Seconds, UndoEntry, UndoId, UndoScope, UndoState, UndoToken,
};
use prov::{Actor, RunId, SessionId, UnixSeconds};

/// The journal.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UndoJournal {
    entries: Vec<UndoEntry>,
}

impl UndoJournal {
    /// An empty journal.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a change that can be undone, and returns its row.
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &mut self,
        at: UnixSeconds,
        actor: Actor,
        action: ActionRef,
        said: LabelText,
        token: UndoToken,
        run: Option<RunId>,
        session: Option<SessionId>,
    ) -> UndoId {
        let id = UndoId(self.entries.last().map_or(1, |e| e.id.0 + 1));
        self.entries.push(UndoEntry {
            id,
            at,
            actor,
            action,
            said,
            token,
            run,
            session,
            state: UndoState::Available,
        });
        id
    }

    /// One row.
    pub fn get(&self, id: UndoId) -> Option<&UndoEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Every row, oldest first.
    pub fn entries(&self) -> &[UndoEntry] {
        &self.entries
    }

    /// Sets a row's state (after an undo, or the app's `UndoChanged`). Returns whether the row
    /// exists.
    pub fn mark(&mut self, id: UndoId, state: UndoState) -> bool {
        self.entries
            .iter_mut()
            .find(|e| e.id == id)
            .map(|e| e.state = state)
            .is_some()
    }

    /// The rows an undo of `scope` would undo, newest first, only those still available. A
    /// task's rows are those of its sessions.
    pub fn plan_undo(&self, scope: &UndoScope, task_sessions: &[SessionId]) -> Vec<UndoId> {
        let inside = |e: &UndoEntry| match scope {
            UndoScope::Entry(id) => e.id == *id,
            UndoScope::Run(run) => e.run.as_ref() == Some(run),
            UndoScope::Task(_) => e
                .session
                .as_ref()
                .is_some_and(|s| task_sessions.contains(s)),
        };
        self.entries
            .iter()
            .rev()
            .filter(|e| e.state == UndoState::Available && inside(e))
            .map(|e| e.id)
            .collect()
    }

    /// Expires the available rows older than `keep` (the setting `agent.undo.keep_h`).
    pub fn expire(&mut self, now: UnixSeconds, keep: Seconds) {
        for e in self
            .entries
            .iter_mut()
            .filter(|e| e.state == UndoState::Available)
        {
            if now.0 - e.at.0 > i64::from(keep.0) {
                e.state = UndoState::Expired;
            }
        }
    }
}
