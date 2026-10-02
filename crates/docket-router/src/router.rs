//! The router: the seams, the configuration and the state, with one entry point.

use crate::handles::HandleTable;
use crate::index::IndexEvent;
use crate::journal::UndoJournal;
use crate::registry::Registry;
use crate::seams::Seams;
use crate::session::SessionState;
use crate::tasks::TaskTable;
use docket_core::{
    AgentConfig, CallerId, Halt, IndexState, IntentsReply, IntentsRequest, KillSwitch,
};
use policy_point::Pdp;
use porter_core::AppName;
use prov::{SessionId, UnixSeconds};
use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Mutex;

/// One session as the router keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    /// Where it stands.
    pub state: SessionState,
    /// What it holds for readers.
    pub handles: HandleTable,
}

/// Everything the router mutates, behind one lock.
#[derive(Debug)]
pub struct RouterState {
    /// The installed manifests.
    pub registry: Registry,
    /// Open sessions.
    pub sessions: BTreeMap<SessionId, SessionRecord>,
    /// Tasks.
    pub tasks: TaskTable,
    /// The undo journal.
    pub journal: UndoJournal,
    /// The halts.
    pub kill: KillSwitch,
    /// Each app's shadow index.
    pub index: BTreeMap<AppName, IndexState>,
}

impl RouterState {
    /// A router that has seen nothing: nothing installed, nothing halted.
    pub fn new() -> Self {
        Self {
            registry: Registry::new(),
            sessions: BTreeMap::new(),
            tasks: TaskTable::new(),
            journal: UndoJournal::new(),
            kill: KillSwitch {
                all: Halt::Running,
                spaces: BTreeMap::new(),
            },
            index: BTreeMap::new(),
        }
    }

    /// Applies an index event for an app and returns the action to take.
    pub fn index_event(&mut self, app: &AppName, event: IndexEvent) -> crate::index::IndexAction {
        let state = self.index.get(app).copied().unwrap_or(IndexState::Unknown);
        let (next, action) = crate::index::index_step(state, event);
        self.index.insert(app.clone(), next);
        action
    }
}

impl Default for RouterState {
    fn default() -> Self {
        Self::new()
    }
}

/// The router over one set of seams.
#[derive(Debug)]
pub struct Router<S: Seams> {
    /// The seams.
    pub seams: S,
    /// The proposed values, as the settings give them.
    pub config: AgentConfig,
    /// The policy point.
    pub pdp: Pdp,
    /// What it mutates.
    pub state: Mutex<RouterState>,
}

impl<S: Seams> Router<S> {
    /// A router with nothing installed.
    pub fn new(seams: S, config: AgentConfig, pdp: Pdp) -> Self {
        Self {
            seams,
            config,
            pdp,
            state: Mutex::new(RouterState::new()),
        }
    }

    /// Answers one request from one caller. The caller's identity is what the transport
    /// derived; the router checks that its role may make the request (`permits`) before
    /// anything else.
    pub fn handle(
        &self,
        caller: &CallerId,
        request: IntentsRequest,
    ) -> impl Future<Output = IntentsReply> + Send {
        let _ = (caller, request, UnixSeconds(0));
        async {
            todo!(
                "Router::handle: permits, then one arm per member over the machines in this crate"
            )
        }
    }
}
