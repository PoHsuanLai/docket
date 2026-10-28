//! Every request of `org.quire.Intents1`.

use crate::bodies::{
    GrantAsk, JournalFilter, NoteAsk, ReadAsk, RecallAsk, SessionModeAsk, StoredAsk, WidenAsk,
};
use crate::task::SessionOpen;
use companion_model::{HandTo, WatchedAgent};
use docket_core::{
    ActivationToken, CheckpointId, HaltCause, InboxAsk, IndexBatch, MessageDraft, Rewind,
    SearchAsk, SuggestAsk, TurnEnd, TurnIn, UndoScope, UserTurn,
};
use docket_gate_wire::CuaAsk;
use docket_vocab::{ActionRef, CallRequest, Handle, TurnId, UndoId, WindowKey, Workspace};
use porter_core::AppName;
use prov::{EntityId, SessionId, SpaceScope};
use serde::{Deserialize, Serialize};

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
    SearchCancel(docket_vocab::Generation),
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
        activation: Option<ActivationToken>,
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
    /// `.Session.SetMode` (the shell and control roles only): switches a conversation between
    /// Auto and Manual without a turn.
    SessionMode(SessionModeAsk),
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
    /// `.Checkpoint.WatchAgent`: `Checkpoint.Watch` for a terminal that also knows which agent
    /// app it is and the app's own id for the conversation, so the person can bring the
    /// conversation into docket later.
    CheckpointWatchAgent {
        /// The folder the watched agent works in.
        workspace: Workspace,
        /// What the person calls the agent here. Display text, never trusted.
        label: String,
        /// Who keeps the agent's own history.
        rewind: Rewind,
        /// The app and its own id for the conversation.
        agent: WatchedAgent,
    },
    /// `.Checkpoint.Handover`: where a session's conversation is now. The host of the agent apps
    /// hands its session to a terminal (`Terminal`: it becomes a watch session that the
    /// terminal marks) and takes it back (`Desktop`). Answers `HandedOver`.
    CheckpointHandover {
        /// The session.
        session: SessionId,
        /// Where the conversation goes.
        to: HandTo,
        /// The app and its own id for the conversation, said by the host when it goes to the
        /// terminal (the router cannot know it); not needed to bring it back.
        agent: Option<WatchedAgent>,
    },
    /// `.Checkpoint.Watched`: the watch sessions whose agent app a terminal named.
    CheckpointWatched,
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
    ControlStandingRevoke(docket_permissions_core::StandingGrantId),
}
