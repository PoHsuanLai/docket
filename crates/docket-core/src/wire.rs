//! The protocol of `org.quire.Intents1`: one request and reply vocabulary with two carriers,
//! as in porter. Over D-Bus each member carries the JSON of the named body in an `s`; the
//! in-process transport hands these enums straight over. The caller's identity is derived from
//! the connection and never sent.

use crate::budget::{HaltCause, KillSwitch};
use crate::call::{CallRefusal, CallRequest, Outcome};
use crate::confirm::ConfirmEnd;
use crate::context::EntityRef;
use crate::context::{ContextView, Reveal};
use crate::gate::{CuaAsk, GateAnswer};
use crate::ids::UndoId;
use crate::ids::{Handle, IntentsVocab, TurnId, WindowKey};
use crate::index::{Hit, IndexBatch, SearchAsk, SuggestAsk};
use crate::message::{Delivery, InboundLine, InboxAsk, MessageDraft, SendRefusal};
use crate::planner::TurnIn;
use crate::preview::Preview;
use crate::reader::ReaderAsk;
use crate::roster::{EpisodeLine, RecalledLine};
use crate::task::{SessionOpen, SessionOpened};
use crate::task_policy::TaskPolicy;
use crate::undo::{UndoEntry, UndoFault, UndoScope};
use crate::value::Value;
use almanac_core::{Episode, EventSummary, InjectQuery, RecentQuery};
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
}

/// One recent event as the planner may read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentLine {
    /// What and when.
    pub summary: EventSummary,
    /// Its effect.
    pub effect: prov::Effect,
    /// Its text: plain when trusted, a handle when not.
    pub text: Option<Reveal<String>>,
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
}

/// An episode the idle pass hands the router to record (`Session.Note`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteAsk {
    /// The episode, with its narrative.
    pub episode: Episode,
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
}

/// Every reply of `org.quire.Intents1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
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
    /// A handle's text (the reader only, or the screen).
    Text(String),
    /// What the reader answered: a plain closed-set value, or a handle.
    Read(Reveal<Value>),
    /// The task policy, if there is one.
    TaskPolicy(Option<Box<TaskPolicy>>),
    /// A widening ended.
    Widened(WidenAnswer),
    /// What memory returned.
    Recalled(RecallView),
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
    /// The request was refused.
    Refused(WireRefusal),
}
