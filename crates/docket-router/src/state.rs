//! What the router keeps: one record per open session, the tasks, the journal, the halts and
//! the shadow index, behind the one lock `Router` holds.

use crate::handles::HandleTable;
use crate::index::IndexEvent;
use crate::journal::UndoJournal;
use crate::registry::Registry;
use crate::session::SessionState;
use crate::tasks::TaskTable;
use action_review::Breaker;
use docket_core::{
    CallerRole, ConfirmId, Halt, IndexEntry, IndexState, KillSwitch, Ledger, SessionSaw, StepLine,
    Strictness, TaskPolicy, UserTurn,
};
use porter_core::{AppName, Count};
use prov::{
    Actor, Address, EntityId, EntityKey, EntityKind, Label, Message, SessionId, SpaceId, TaskId,
    UnixSeconds,
};
use std::collections::{BTreeMap, BTreeSet};

use docket_core::Saw;

/// One session as the router keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    /// Where it stands.
    pub state: SessionState,
    /// What it holds for readers.
    pub handles: HandleTable,
    /// The task it runs.
    pub task: TaskId,
    /// Who acts in it, as calls and the undo journal name them.
    pub actor: Actor,
    /// The app that opened it.
    pub opener: AppName,
    /// Its Space.
    pub space: SpaceId,
    /// What it has used.
    pub ledger: Ledger,
    /// How its calls have been decided.
    pub breaker: Breaker,
    /// What it has seen, computed here and never reported by a model.
    pub saw: SessionSaw,
    /// The person's own words, as recorded.
    pub turns: Vec<UserTurn>,
    /// What this task may do.
    pub policy: Option<TaskPolicy>,
    /// How its calls ended.
    pub history: Vec<StepLine>,
    /// Things the router showed it, so a name for one is not a model's invention.
    pub known: BTreeSet<EntityId>,
    /// The distinct labels of everything delivered to it: a message it sends carries their join.
    pub seen: Vec<Label>,
    /// Messages that landed for it and have not been read.
    pub inbox: Vec<Message>,
}

impl SessionRecord {
    /// A session that has seen nothing and used nothing.
    pub fn new(
        task: TaskId,
        actor: Actor,
        opener: AppName,
        space: SpaceId,
        now: UnixSeconds,
    ) -> Self {
        Self {
            state: SessionState::Open(crate::session::Taint::Clean),
            handles: HandleTable::new(),
            task,
            actor,
            opener,
            space,
            ledger: Ledger::new(now),
            breaker: Breaker::new(),
            saw: SessionSaw {
                private: Saw::NotSeen,
                untrusted: Saw::NotSeen,
            },
            turns: Vec::new(),
            policy: None,
            history: Vec::new(),
            known: BTreeSet::new(),
            seen: Vec::new(),
            inbox: Vec::new(),
        }
    }
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
    /// The entries each app pushed.
    pub shadow: BTreeMap<AppName, BTreeMap<(EntityKind, EntityKey), IndexEntry>>,
    /// How much each Space asks; a Space not listed uses the configured default.
    pub strictness: BTreeMap<SpaceId, Strictness>,
    /// The session an app or MCP client that opened none acts in.
    pub implicit: BTreeMap<(CallerRole, AppName), SessionId>,
    /// Messages for a party with no session of its own (the person), by address.
    pub inboxes: BTreeMap<Address, Vec<Message>>,
    /// Confirmations on a sheet right now, by Space, so a halt can withdraw them.
    pub pending: BTreeMap<ConfirmId, SpaceId>,
    /// The counter every minted id draws from.
    pub minted: Count,
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
                audit_lost: Count(0),
            },
            index: BTreeMap::new(),
            shadow: BTreeMap::new(),
            strictness: BTreeMap::new(),
            implicit: BTreeMap::new(),
            inboxes: BTreeMap::new(),
            pending: BTreeMap::new(),
            minted: Count(0),
        }
    }

    /// Applies an index event for an app and returns the action to take.
    pub fn index_event(&mut self, app: &AppName, event: IndexEvent) -> crate::index::IndexAction {
        let state = self.index.get(app).copied().unwrap_or(IndexState::Unknown);
        let (next, action) = crate::index::index_step(state, event);
        self.index.insert(app.clone(), next);
        action
    }

    /// The next number for an id (a call, a session, a message, a confirmation).
    pub fn mint(&mut self) -> u32 {
        self.minted = Count(self.minted.0.saturating_add(1));
        self.minted.0
    }
}

impl Default for RouterState {
    fn default() -> Self {
        Self::new()
    }
}
