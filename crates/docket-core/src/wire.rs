//! The protocol of `org.quire.Intents1`: one request and reply vocabulary with two carriers,
//! as in porter. Over D-Bus each member carries the JSON of the named body in an `s`; the
//! in-process transport hands these enums straight over. The caller's identity is derived from
//! the connection and never sent.

use crate::budget::{HaltCause, KillSwitch};
use crate::call::{CallRefusal, CallRequest, Outcome};
use crate::checkpoint::{
    CheckpointFault, CheckpointId, CheckpointList, RestorePlan, Rewind, TurnEnd,
};
use crate::confirm::ConfirmEnd;
use crate::context::EntityRef;
use crate::context::{ContextView, Reveal};
use crate::gate::{CuaAsk, GateAnswer};
use crate::ids::{ActionRef, UndoId};
use crate::ids::{Handle, IntentsVocab, TurnId, WindowKey};
use crate::index::{Hit, IndexBatch, SearchAsk, SuggestAsk};
use crate::message::{Delivery, InboundLine, InboxAsk, MessageDraft, SendRefusal};
use crate::planner::TurnIn;
use crate::planner::{HandleCard, UserTurn};
use crate::preview::Preview;
use crate::reader::ReaderAsk;
use crate::roster::{EpisodeLine, RecalledLine};
use crate::roster::{PrimerText, ProfileLine};
use crate::task::{SessionOpen, SessionOpened};
use crate::task_policy::TaskPolicy;
use crate::undo::{UndoEntry, UndoFault, UndoScope};
use crate::value::Value;
use crate::workspace::Workspace;
use almanac_core::{
    Episode, EpisodeId, EventSummary, InjectQuery, JsonText, Narrative, RecentQuery,
};
use porter_core::{AppName, Count};
use prov::{EntityId, RunId, SessionId, SpaceId, SpaceScope};
use serde::{Deserialize, Serialize};

/// A body with the vocabulary it is written in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope<T> {
    /// The vocabulary.
    pub vocab: IntentsVocab,
    /// The body.
    pub body: T,
}

/// The planner asks the reader (`Session.Read`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadAsk {
    /// The ask.
    pub ask: ReaderAsk,
}

/// A policy change that needs the person (`Session.Widen`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidenAsk {
    /// The turn whose words the person confirms against.
    pub turn: TurnId,
    /// The wider policy.
    pub change: TaskPolicy,
}

/// How a widening ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum WidenAnswer {
    /// The person confirmed: the wider policy is active.
    Applied,
    /// The old policy stays.
    Refused(ConfirmEnd),
}

/// What the router reads from memory for a companion session (`Session.Recall`): the router
/// applies the labels, so untrusted text comes back as handles and the planner never reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum RecallAsk {
    /// Recent activity in the session's Space.
    Recent(RecentQuery),
    /// Automatic top-k recall for this turn.
    Inject(InjectQuery),
    /// The Space's recent episodes as the planner reads them (the query's kinds and bodies are
    /// the router's: `companion.episode`, with bodies): the skeleton as trusted lines, a
    /// narrative by handle.
    Episodes(RecentQuery),
    /// The Space's primer: at most 200 lines of what is kept.
    Primer,
    /// The facts the person stated themselves at the desktop scope.
    Profile,
    /// The Spaces a restart should read: every Space memory keeps records for, and the ones the
    /// router itself holds a session or a task in. Asked in any session (the Space it is in does
    /// not matter).
    Spaces,
}

/// One recent event as the planner may read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentLine {
    /// What and when.
    pub summary: EventSummary,
    /// Its effect.
    pub effect: prov::Effect,
    /// Its provenance.
    pub label: prov::Label,
    /// Its text: plain when trusted, a handle when not.
    pub text: Option<Reveal<String>>,
    /// The body's serde JSON, when the query asked for bodies and the label is trusted: an
    /// untrusted body never reaches the companion (its text is the handle above).
    pub body: Option<JsonText>,
}

/// What a recall returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum RecallView {
    /// Recent events.
    Recent(Vec<RecentLine>),
    /// Recalled items.
    Hits(Vec<RecalledLine>),
    /// Recent episodes, skeleton as trusted lines.
    Episodes(Vec<EpisodeLine>),
    /// The Space's primer.
    Primer(PrimerText),
    /// The person's own pinned facts.
    Profile(Vec<ProfileLine>),
    /// The Spaces to read on a restart, each once.
    Spaces(Vec<prov::SpaceId>),
}

/// The tail of a session record's kind (`companion.session.<slug>`): lowercase words and digits
/// with underscores, 1 to 32 characters, starting with a letter.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NoteSlug(String);

/// Why a slug is not one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("a note slug is 1 to 32 lowercase letters, digits and underscores, starting with a letter")]
pub struct NoteSlugError;

impl NoteSlug {
    /// `text` as a slug.
    pub fn parse(text: &str) -> Result<Self, NoteSlugError> {
        let mut chars = text.chars();
        let first = chars.next().is_some_and(|c| c.is_ascii_lowercase());
        let rest = chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if first && rest && text.len() <= 32 {
            Ok(Self(text.to_owned()))
        } else {
            Err(NoteSlugError)
        }
    }

    /// The slug.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for NoteSlug {
    type Error = NoteSlugError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<NoteSlug> for String {
    fn from(slug: NoteSlug) -> String {
        slug.0
    }
}

/// A record companiond keeps about its own sessions (`companion-wire`'s `SessionRecord`, in its
/// serde form): the router stores it for the Space the session is in, as
/// `Area { Companion }` of kind `companion.session.<slug>`, trusted and private to the Space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionNote {
    /// What kind of record: the tail of its kind tag.
    pub slug: NoteSlug,
    /// The record's serde JSON.
    pub json: JsonText,
}

/// What the companion hands the router to record (`Session.Note`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum NoteAsk {
    /// An episode as it is: a side conversation's, or a worker's skeleton when its final report
    /// ended the task before the router could write one.
    Episode(Box<Episode>),
    /// The idle pass's narrative of an episode the router already holds the skeleton of: the
    /// router merges it into its own skeleton (the companion never restates the skeleton) and
    /// records the narrated successor.
    Narrative {
        /// Which episode (the task's id).
        episode: EpisodeId,
        /// What the model wrote.
        narrative: Narrative,
    },
    /// The task of this session is over: the router ends it and leaves its skeleton now, as
    /// closing the session would, but the session stays open (its handles can still be shown)
    /// until `Session.Close`. A task already ended writes nothing.
    End,
    /// A record of the companion's sessions.
    Record(SessionNote),
}

/// What to list of the undo journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalFilter {
    /// Only this run.
    pub run: Option<RunId>,
    /// Only this session.
    pub session: Option<SessionId>,
    /// At most this many, newest first.
    pub limit: Count,
}

/// The CUA consent grant (`Gate.Grant`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantAsk {
    /// The app.
    pub app: AppName,
    /// The Space.
    pub space: SpaceId,
}

/// How a grant request ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum GrantAnswer {
    /// The person allowed it.
    Granted,
    /// They did not.
    Refused(ConfirmEnd),
}

/// How far an undo got.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoReport {
    /// How many entries were undone.
    pub undone: Count,
    /// Why it stopped early, if it did.
    pub stopped: Option<UndoFault>,
}

/// Every request of `org.quire.Intents1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
#[non_exhaustive]
pub enum IntentsRequest {
    /// `.Registry.Manifests`.
    Manifests,
    /// `.Index.Push`.
    IndexPush(IndexBatch),
    /// `.Index.Reset`.
    IndexReset {
        /// The epoch to start.
        epoch: u64,
    },
    /// `.Search.Query`.
    Search(SearchAsk),
    /// `.Search.Cancel`.
    SearchCancel(crate::units::Generation),
    /// `.Run.Perform`.
    Perform {
        /// The call.
        call: CallRequest,
        /// The session the call belongs to, for a companion with several tasks working at once;
        /// `None` for the person's own surfaces and for callers with one session.
        session: Option<SessionId>,
        /// The window to anchor a confirmation to.
        parent_window: Option<WindowKey>,
        /// The launcher's activation token (D-Bus option `activation`), passed to the app's
        /// `Perform`. The router drops it for every other role.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        activation: Option<crate::call::ActivationToken>,
    },
    /// `.Run.DryRun`: what the app would change, through the same arguments check, labels and
    /// policy as `Perform`, without asking the person, dispatching or charging a budget. A call
    /// policy refuses is refused here with the same refusal.
    DryRun {
        /// The call.
        call: CallRequest,
        /// The session, as in `Perform`.
        session: Option<SessionId>,
    },
    /// `.Run.Preview`.
    Preview(EntityId),
    /// `.Run.Suggest`.
    Suggest(SuggestAsk),
    /// `.Run.Undo`.
    Undo(UndoId),
    /// `.Run.UndoAll`.
    UndoAll(UndoScope),
    /// `.Context.Current`.
    Context {
        /// The session.
        session: SessionId,
        /// The app whose window the person is in: the one the companion was summoned from.
        app: AppName,
    },
    /// `.Session.Open`.
    SessionOpen(SessionOpen),
    /// `.Session.Turn`.
    SessionTurn {
        /// The session.
        session: SessionId,
        /// What the person said.
        turn: TurnIn,
    },
    /// `.Session.Close`.
    SessionClose {
        /// The session.
        session: SessionId,
    },
    /// `.Session.TurnEnded`: the host says the agent's turn is over, however it ended. It can
    /// only mark a turn ended; it never starts, allows or skips anything else.
    SessionTurnEnded {
        /// The session.
        session: SessionId,
        /// The turn that ended.
        turn: TurnId,
        /// How it ended.
        how: TurnEnd,
    },
    /// `.Session.Resolve` (the reader only).
    SessionResolve {
        /// The session.
        session: SessionId,
        /// The handle.
        handle: Handle,
    },
    /// `.Session.Display` (the launcher and the summoning app only: text for the screen).
    SessionDisplay {
        /// The session.
        session: SessionId,
        /// The handle.
        handle: Handle,
    },
    /// `.Session.DisplayLabelled` (the same roles as `SessionDisplay`): the text for the screen
    /// with the label the session holds for it, so the screen can show how far to trust it.
    SessionDisplayLabelled {
        /// The session.
        session: SessionId,
        /// The handle.
        handle: Handle,
    },
    /// `.Session.Read`.
    SessionRead {
        /// The session.
        session: SessionId,
        /// The ask.
        ask: ReadAsk,
    },
    /// `.Session.TaskPolicy`.
    SessionTaskPolicy {
        /// The session.
        session: SessionId,
    },
    /// `.Session.Widen`.
    SessionWiden {
        /// The session.
        session: SessionId,
        /// The change.
        widen: WidenAsk,
    },
    /// `.Session.Note`.
    SessionNote {
        /// The session.
        session: SessionId,
        /// The episode.
        note: NoteAsk,
    },
    /// `.Session.Recall`.
    SessionRecall {
        /// The session.
        session: SessionId,
        /// What to read.
        ask: RecallAsk,
    },
    /// `.Session.Narrow` (the companion only): narrows a session's policy from the person's
    /// words, through the policy writer. Never wider than the session's own.
    SessionNarrow {
        /// The session.
        session: SessionId,
        /// What the person said to its agent.
        turn: UserTurn,
    },
    /// `.Session.Handles` (the companion only): what the session holds by handle, in shape only.
    SessionHandles {
        /// The session.
        session: SessionId,
    },
    /// `.Session.Stored`: the durable log of sessions, for an edge that lists, loads or forks
    /// them. The router answers only for sessions the caller may bring back.
    SessionStored {
        /// What to read.
        ask: StoredAsk,
    },
    /// `.Checkpoint.List`: a session's restore points, oldest first, joined with what the store
    /// still holds. Never for an agent: it cannot read its own restore points.
    CheckpointList {
        /// The session.
        session: SessionId,
    },
    /// `.Checkpoint.Plan`: what restoring one point would change. Read-only; the restore itself
    /// is the destructive action `checkpoints.restore`, through the gate.
    CheckpointPlan {
        /// The session.
        session: SessionId,
        /// The point.
        id: CheckpointId,
    },
    /// `.Checkpoint.Watch`: opens a session that only keeps restore points, for an agent the
    /// caller watches but does not host (a CLI agent in a terminal pane). The session has no
    /// task, planner or policy and reaches no action: it can only be marked, listed, planned,
    /// restored and closed. Only a terminal may ask, and only it may use the session after.
    CheckpointWatch {
        /// The folder the watched agent works in.
        workspace: Workspace,
        /// What the person calls the agent here ("claude in pane 2"). Display text, never trusted.
        label: String,
        /// Who keeps the agent's own history (`Agent`: docket saves nothing and says so).
        rewind: Rewind,
    },
    /// `.Checkpoint.Mark`: the watched agent started working. Takes a restore point (unless the
    /// agent keeps its own) and starts the running turn, answering `TurnRecorded` with the number
    /// of that turn; `Session.TurnEnded` for it ends it. Records no words of the person's.
    CheckpointMark {
        /// The watch session.
        session: SessionId,
    },
    /// `.Message.Send`.
    MessageSend {
        /// The session the sender speaks from.
        session: SessionId,
        /// The draft.
        draft: MessageDraft,
    },
    /// `.Message.Inbox`.
    MessageInbox(InboxAsk),
    /// `.Gate.Grant`.
    GateGrant(GrantAsk),
    /// `.Gate.Check`.
    GateCheck(CuaAsk),
    /// `.Control.Halt`.
    ControlHalt {
        /// Which Spaces.
        scope: SpaceScope,
        /// Why.
        cause: HaltCause,
    },
    /// `.Control.Resume`.
    ControlResume {
        /// Which Spaces.
        scope: SpaceScope,
    },
    /// `.Control.State`.
    ControlState,
    /// `.Control.Journal`.
    ControlJournal(JournalFilter),
    /// `.Control.TerminalGrants`: the actions the terminal may run without asking, until logout.
    ControlTerminalGrants,
    /// `.Control.RevokeTerminalGrant`: the next call of this action from the terminal asks again.
    ControlTerminalRevoke(ActionRef),
    /// `.Control.StandingGrants`: the standing "allow always" grants held (Settings lists them).
    ControlStandingGrants,
    /// `.Control.RevokeStandingGrant`: the next call it covered asks again.
    ControlStandingRevoke(crate::StandingGrantId),
}

/// Why a request got no answer of its own kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum WireRefusal {
    /// This caller's role may not make this request.
    NotAllowed,
    /// The session is unknown or closed.
    NoSuchSession,
    /// The request is malformed.
    Malformed,
    /// A call was refused.
    Call(CallRefusal),
    /// A message was not delivered.
    Send(SendRefusal),
    /// A read gave no answer.
    Read(crate::reader::ReadFault),
}

/// A handle's content as the reader receives it: the text and the label the router holds for it,
/// so the reader classes what it sends to a model by the handle's own class (mail, not "the
/// person's words").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolved {
    /// The text.
    pub text: String,
    /// Its label in the session's table.
    pub label: prov::Label,
}

/// A handle's content as the screen receives it: the text, and the label the router holds for it
/// so the screen can mark content that is not the person's own. Never for a model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Displayed {
    /// The text.
    pub text: String,
    /// Its label in the session's table.
    pub label: prov::Label,
}

/// What `.Session.Stored` is asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum StoredAsk {
    /// The sessions the caller may bring back, oldest first.
    List,
    /// One session's rows from a position, at most `size`.
    Rows {
        /// The session.
        session: SessionId,
        /// The position to start from (the first row when none).
        from: Option<u64>,
        /// How many rows at most.
        size: u32,
    },
    /// A new session that keeps `session`'s log up to and including the row `at` (the router
    /// writes the child's log; the caller may bring the parent back, as for `Rows`).
    Fork {
        /// The session to fork.
        session: SessionId,
        /// The last row the fork keeps.
        at: u64,
    },
}

/// One row of a stored session: its position and its body, which `docket-session` writes and reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRow {
    /// The position.
    pub seq: u64,
    /// The row as JSON.
    pub json: String,
}

/// What `.Session.Stored` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum StoredView {
    /// The sessions.
    Sessions(Vec<SessionId>),
    /// A page of rows.
    Rows {
        /// The rows, oldest first.
        rows: Vec<StoredRow>,
        /// Where the next page starts; none at the end.
        next: Option<u64>,
    },
    /// The session a fork made.
    Forked(SessionId),
}

/// Every reply of `org.quire.Intents1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
#[non_exhaustive]
pub enum IntentsReply {
    /// Nothing to return.
    Done,
    /// The installed manifests.
    Manifests(Vec<crate::validate::ValidManifest>),
    /// Search hits.
    Hits(Vec<Hit>),
    /// How a call ended.
    Performed(Box<Result<Outcome, CallRefusal>>),
    /// A preview.
    Preview(Preview),
    /// Suggested things.
    Suggestions(Vec<EntityRef>),
    /// An undo of one entry.
    Undone(Result<(), UndoFault>),
    /// An undo of several.
    UndoneAll(UndoReport),
    /// What the person is doing, as a planner may read it.
    Context(Box<ContextView>),
    /// A session was opened.
    SessionOpened(SessionOpened),
    /// A turn was recorded.
    TurnRecorded(TurnId),
    /// A handle's text for the screen (`Session.Display`).
    Text(String),
    /// A handle's text and its label, for the quarantined reader (`Session.Resolve`).
    Resolved(Resolved),
    /// A handle's text and its label, for the screen (`Session.DisplayLabelled`).
    Displayed(Displayed),
    /// What the reader answered: a plain closed-set value, or a handle.
    Read(Reveal<Value>),
    /// The task policy, if there is one.
    TaskPolicy(Option<Box<TaskPolicy>>),
    /// A widening ended.
    Widened(WidenAnswer),
    /// What memory returned.
    Recalled(RecallView),
    /// What a session holds by handle: shape, source and size, never the content.
    Handles(Vec<HandleCard>),
    /// What the durable log of sessions holds.
    Stored(StoredView),
    /// A session's restore points.
    Checkpoints(Box<CheckpointList>),
    /// What restoring a point would change, or why that cannot be said.
    CheckpointPlan(Result<RestorePlan, CheckpointFault>),
    /// A watch session was opened (`Checkpoint.Watch`).
    CheckpointWatching(SessionId),
    /// A message was delivered.
    Delivered(Delivery),
    /// Messages that wait.
    Inbox(Vec<InboundLine>),
    /// A grant request ended.
    Granted(GrantAnswer),
    /// The gate's answer.
    Gate(GateAnswer),
    /// The kill switch.
    State(KillSwitch),
    /// The journal.
    Journal(Vec<UndoEntry>),
    /// The actions the terminal holds a standing grant for.
    TerminalGrants(Vec<ActionRef>),
    /// The standing grants held.
    StandingGrants(Vec<crate::StandingGrant>),
    /// The request was refused.
    Refused(WireRefusal),
}
