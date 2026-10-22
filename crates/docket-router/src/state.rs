//! What the router keeps: one record per open session, the tasks, the journal, the halts and
//! the shadow index, behind the one lock `Router` holds.

use crate::handles::{HandleLimit, HandleTable, MAX_KNOWN};
use crate::index::IndexEvent;
use crate::journal::UndoJournal;
use crate::registry::Registry;
use crate::session::SessionState;
use crate::tasks::TaskTable;
use crate::wal::{Lane, Wal};
use action_review::Breaker;
use docket_core::{
    CallFacts, CallerRole, CheckpointNote, ConfirmId, ExternalAgent, Halt, IndexEntry, IndexState,
    KillSwitch, Ledger, Mode, Rewind, SessionSaw, StepLine, TaskPolicy, UserTurn,
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
    /// How its changes are handled: Auto or Manual. The person (the shell) switches it; a child
    /// session follows its parent's through [`RouterState::mode_of`].
    pub mode: Mode,
    /// The conversation this session works for, whose mode it follows: a worker, a helper job or
    /// a screen run takes the mode of the session that started it. None for a conversation.
    pub parent: Option<SessionId>,
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
    /// The briefs the person confirmed on a sheet, oldest first. Kept in memory: a restart forgets
    /// them and the planner must ask again, which is the safe direction.
    pub briefs: Vec<crate::brief::ConfirmedBrief>,
    /// What this task may do.
    pub policy: Option<TaskPolicy>,
    /// How its calls ended.
    pub history: Vec<StepLine>,
    /// Things the router showed it, so a name for one is not a model's invention.
    pub known: BTreeSet<EntityId>,
    /// The distinct labels of everything delivered to it: a message it sends carries their join.
    pub seen: Vec<Label>,
    /// The apps the person allowed this screen run to use (the start sheet), for a session that
    /// is a screen run's. It lives and ends with the session.
    pub screen_apps: BTreeSet<AppName>,
    /// The apps the person let this conversation's screen runs use (a start sheet answered
    /// Allow), for a session that is a conversation: a later run of the conversation in the same
    /// app is not asked again. It lives and ends with the session.
    pub conversation_apps: BTreeSet<AppName>,
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
    /// What the person calls the agent a watch session watches, as its opener said (display
    /// text); none for every other session.
    pub label: Option<prov::AgentLabel>,
    /// The helper whose job the session is for: its reach cuts the session's policy, and the
    /// things the job owns are written by no other session while it lives. Said by the companion
    /// when it opened the session.
    pub helper: Option<docket_core::helper::HelperSeat>,
    /// The agent app's own id for the conversation, while the session is a watch session that
    /// came from an agent app (a terminal named it, or the host handed the conversation to a
    /// terminal). Said by the host or the terminal, never read from a model.
    pub agent_session: Option<docket_core::AppSessionId>,
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
            mode: Mode::Auto,
            parent: None,
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
            briefs: Vec::new(),
            policy: None,
            history: Vec::new(),
            known: BTreeSet::new(),
            seen: Vec::new(),
            inbox: Vec::new(),
            screen_apps: BTreeSet::new(),
            conversation_apps: BTreeSet::new(),
            skill_loads: docket_skills::Loaded::default(),
            wal: Wal::Off,
            external: None,
            cwd: None,
            approvals: Vec::new(),
            checkpoints: Vec::new(),
            kind: SessionKind::Agent,
            label: None,
            helper: None,
            agent_session: None,
        }
    }

    /// Whether `role` may speak in this session without having opened it: an editor, in a
    /// helper's conversation, which the companion opened for the person to talk in. A helper's
    /// job is never one, and the seat is the companion's to give, not the editor's.
    pub fn is_conversation_for(&self, role: CallerRole) -> bool {
        role == CallerRole::Editor
            && self
                .helper
                .as_ref()
                .is_some_and(|seat| seat.place == docket_core::helper::SeatPlace::Conversation)
    }

    /// Notes things the router showed this session, so a name for one is not a model's
    /// invention. Past [`MAX_KNOWN`] a new thing is refused and none after it is noted.
    pub fn know(&mut self, things: impl IntoIterator<Item = EntityId>) -> Result<(), HandleLimit> {
        for thing in things {
            if self.known.len() >= MAX_KNOWN && !self.known.contains(&thing) {
                return Err(HandleLimit::Known);
            }
            self.known.insert(thing);
        }
        Ok(())
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
