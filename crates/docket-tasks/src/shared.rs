//! What the bus reads without waiting for the companion: the roster, the front task and each
//! answer as of the last effect, and the two things that reach the running loop from outside
//! (a cancel, and an interactive request that the idle pass must yield to). A planner turn can
//! sit on a confirmation for minutes; `Roster()` and `Front()` must not.

use crate::seams::Surface;
use companion_wire::{AnswerWire, FrontTask, RouteNote};
use docket_core::Roster;
use prov::{SessionId, TaskId};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

/// What changed, content-free: a reader asks for the content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// An answer object appeared.
    AnswerAdded(TaskId),
    /// An answer changed.
    AnswerChanged(TaskId),
    /// An answer object went away.
    AnswerRemoved(TaskId),
    /// The roster changed.
    RosterChanged,
}

#[derive(Debug, Default)]
struct State {
    roster: Roster,
    front: Option<FrontTask>,
    answers: BTreeMap<TaskId, AnswerWire>,
    sessions: BTreeMap<TaskId, SessionId>,
    routes: BTreeMap<TaskId, Vec<RouteNote>>,
    cancels: BTreeSet<TaskId>,
}

/// The shared view, over the surface that hears its changes.
#[derive(Debug, Default)]
pub struct Shared<S> {
    state: Mutex<State>,
    surface: S,
}

impl<S: Surface> Shared<S> {
    /// An empty view over `surface`.
    pub fn new(surface: S) -> Self {
        Self {
            state: Mutex::new(State::default()),
            surface,
        }
    }

    /// The surface.
    pub fn surface(&self) -> &S {
        &self.surface
    }

    fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        match self.state.lock() {
            Ok(mut state) => f(&mut state),
            Err(poisoned) => f(&mut poisoned.into_inner()),
        }
    }

    /// Who is subscribed to the changes.
    pub fn subscribe(&self) -> S::Changes {
        self.surface.changes()
    }

    fn tell(&self, change: Change) {
        self.surface.changed(change);
    }

    /// The roster as of the last effect.
    pub fn roster(&self) -> Roster {
        self.with(|s| s.roster.clone())
    }

    /// Replaces the roster, and says so when it differs.
    pub fn set_roster(&self, roster: Roster) {
        let changed = self.with(|s| {
            let changed = s.roster != roster;
            s.roster = roster;
            changed
        });
        if changed {
            self.tell(Change::RosterChanged);
        }
    }

    /// The front task as of the last effect.
    pub fn front(&self) -> FrontTask {
        self.with(|s| s.front.clone()).unwrap_or(FrontTask {
            task: None,
            session: None,
        })
    }

    /// Sets the front task.
    pub fn set_front(&self, front: FrontTask) {
        self.with(|s| s.front = Some(front));
    }

    /// One answer as of the last effect.
    pub fn answer(&self, task: &TaskId) -> Option<AnswerWire> {
        self.with(|s| s.answers.get(task).cloned())
    }

    /// Sets an answer, saying whether it is new or changed.
    pub fn set_answer(&self, answer: AnswerWire) {
        let task = answer.task.clone();
        let was = self.with(|s| s.answers.insert(task.clone(), answer.clone()));
        match was {
            None => self.tell(Change::AnswerAdded(task)),
            Some(old) if old != answer => self.tell(Change::AnswerChanged(task)),
            Some(_) => {}
        }
    }

    /// The router session an answer's task runs on: the one whose handles the answer shows.
    pub fn session_of(&self, task: &TaskId) -> Option<SessionId> {
        self.with(|s| s.sessions.get(task).cloned())
    }

    /// Records the session of `task`'s answer. Set before the answer, so a reader told of the
    /// answer can already ask for it.
    pub fn set_session(&self, task: &TaskId, session: SessionId) {
        self.with(|s| s.sessions.insert(task.clone(), session));
    }

    /// How the answer's last turn was reached and why (`Answer.Routing`).
    pub fn route_of(&self, task: &TaskId) -> Option<Vec<RouteNote>> {
        self.with(|s| s.routes.get(task).cloned())
    }

    /// Records the route notes of `task`'s answer. Set before the answer, like the session.
    pub fn set_route(&self, task: &TaskId, notes: Vec<RouteNote>) {
        self.with(|s| s.routes.insert(task.clone(), notes));
    }

    /// Drops an answer.
    pub fn drop_answer(&self, task: &TaskId) {
        self.with(|s| s.routes.remove(task));
        self.with(|s| s.sessions.remove(task));
        if self.with(|s| s.answers.remove(task)).is_some() {
            self.tell(Change::AnswerRemoved(task.clone()));
        }
    }

    /// The person (or the answer object's `Cancel`) stopped this task: the loop sees it between
    /// effects.
    pub fn cancel(&self, task: &TaskId) {
        self.with(|s| s.cancels.insert(task.clone()));
    }

    /// Whether a cancel is waiting for `task`, taking it.
    pub fn take_cancel(&self, task: &TaskId) -> bool {
        self.with(|s| s.cancels.remove(task))
    }

    /// An interactive request is starting: whatever background work runs yields now.
    pub fn interrupt(&self) {
        self.surface.interrupt();
    }

    /// Resolves when an interactive request starts.
    pub async fn interrupted(&self) {
        self.surface.interrupted().await;
    }
}
