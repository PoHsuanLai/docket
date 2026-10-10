//! What the router keeps: one record per open session, the tasks, the journal, the halts and
//! the shadow index, behind the one lock `Router` holds.

use crate::handles::HandleTable;
use crate::index::IndexEvent;
use crate::journal::UndoJournal;
use crate::registry::Registry;
use crate::session::SessionState;
use crate::tasks::TaskTable;
use crate::wal::{Lane, Wal};
use action_review::Breaker;
use docket_core::{
    CallFacts, CallerRole, CheckpointNote, ConfirmId, ExternalAgent, Halt, IndexEntry, IndexState,
    KillSwitch, Ledger, Rewind, SessionSaw, StepLine, Strictness, TaskPolicy, UserTurn,
};
use porter_core::{AppName, Count};
use prov::{
    Actor, Address, EntityId, EntityKey, EntityKind, Label, Message, SessionId, SpaceId, TaskId,
    UnixSeconds,
};
use std::collections::{BTreeMap, BTreeSet};

use docket_core::Saw;

/// What a session is for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SessionKind {
    /// An agent's or an app's session: it takes turns and calls.
    #[default]
    Agent,
    /// Only restore points, for an agent a terminal watches (`Checkpoint.Watch`): no task, no
    /// planner, no policy, no action. `Rewind` is who keeps the agent's own history.
    Watch(Rewind),
}

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
    /// The skills this task has loaded (`companion.skill.load`), at most three.
    pub skill_loads: docket_skills::Loaded,
    /// What waits to go on the session's durable record.
    pub wal: Wal,
    /// The external agent program the session is for, as the host that launched it said when it
    /// opened the session; none for every other session.
    pub external: Option<ExternalAgent>,
    /// The directory the session was opened in; a relative path the person wrote means a path
    /// under it.
    pub cwd: Option<docket_core::Workspace>,
    /// Permission requests the person said yes to, each good for one matching call; cleared when
    /// the person speaks again.
    pub approvals: Vec<CallFacts>,
    /// What the session's log says about its restore points (taken, skipped, restored), oldest
    /// first: where the next point's number and the list come from.
    pub checkpoints: Vec<CheckpointNote>,
    /// What the session is for.
    pub kind: SessionKind,
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
            skill_loads: docket_skills::Loaded::default(),
            wal: Wal::Off,
            external: None,
            cwd: None,
            approvals: Vec::new(),
            checkpoints: Vec::new(),
            kind: SessionKind::Agent,
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
    /// The sheets open now, with the calls waiting on each.
    pub(crate) asks: crate::asks::Asks,
    /// The counter every minted id draws from.
    pub minted: Count,
    /// The installed skills (valid files; which are offered is checked against the registry at
    /// each load).
    pub skills: Vec<docket_skills::Skill>,
    /// The writer of each recorded session's log.
    pub(crate) lanes: BTreeMap<SessionId, Lane>,
    /// The sessions whose workspace is being saved or restored right now.
    pub(crate) stepping: BTreeSet<SessionId>,
    /// The turn each session's agent is working on now.
    pub(crate) running: BTreeMap<SessionId, crate::turn_running::RunningTurn>,
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
            asks: crate::asks::Asks::default(),
            minted: Count(0),
            skills: Vec::new(),
            lanes: BTreeMap::new(),
            stepping: BTreeSet::new(),
            running: BTreeMap::new(),
        }
    }

    /// Makes sure no id minted from now on is `number` or below: a restored session brings its
    /// own numbers (ids, turns, calls) back, and a new one must not take them again.
    pub fn reserve(&mut self, number: u64) {
        let number = u32::try_from(number).unwrap_or(u32::MAX);
        self.minted = Count(self.minted.0.max(number));
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
