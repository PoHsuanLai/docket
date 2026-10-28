//! Which member of `Intents1` a request is.

use crate::caller::Member;
use crate::request::IntentsRequest;

impl IntentsRequest {
    /// The member this request calls.
    pub fn member(&self) -> Member {
        use IntentsRequest as R;
        match self {
            R::Manifests => Member::Manifests,
            R::IndexPush(_) => Member::IndexPush,
            R::IndexReset { .. } => Member::IndexReset,
            R::Search(_) => Member::SearchQuery,
            R::SearchCancel(_) => Member::SearchCancel,
            R::Perform { .. } => Member::Perform,
            R::DryRun { .. } => Member::DryRun,
            R::Preview(_) => Member::Preview,
            R::Suggest(_) => Member::Suggest,
            R::Undo(_) => Member::Undo,
            R::UndoAll(_) => Member::UndoAll,
            R::Context { .. } => Member::Context,
            R::SessionOpen(_) => Member::SessionOpen,
            R::SessionTurn { .. } => Member::SessionTurn,
            R::SessionClose { .. } => Member::SessionClose,
            R::SessionTurnEnded { .. } => Member::SessionTurnEnded,
            R::SessionMode(_) => Member::SessionMode,
            R::SessionResolve { .. } => Member::SessionResolve,
            R::SessionDisplay { .. } => Member::SessionDisplay,
            R::SessionDisplayLabelled { .. } => Member::SessionDisplayLabelled,
            R::SessionRead { .. } => Member::SessionRead,
            R::SessionTaskPolicy { .. } => Member::SessionTaskPolicy,
            R::SessionWiden { .. } => Member::SessionWiden,
            R::SessionNote { .. } => Member::SessionNote,
            R::SessionRecall { .. } => Member::SessionRecall,
            R::SessionNarrow { .. } => Member::SessionNarrow,
            R::SessionHandles { .. } => Member::SessionHandles,
            R::SessionStored { .. } => Member::SessionStored,
            R::CheckpointList { .. } => Member::CheckpointList,
            R::CheckpointPlan { .. } => Member::CheckpointPlan,
            R::CheckpointWatch { .. } => Member::CheckpointWatch,
            R::CheckpointMark { .. } => Member::CheckpointMark,
            R::CheckpointWatchAgent { .. } => Member::CheckpointWatchAgent,
            R::CheckpointHandover { .. } => Member::CheckpointHandover,
            R::CheckpointWatched => Member::CheckpointWatched,
            R::MessageSend { .. } => Member::MessageSend,
            R::MessageInbox(_) => Member::MessageInbox,
            R::GateGrant(_) => Member::GateGrant,
            R::GateCheck(_) => Member::GateCheck,
            R::ControlHalt { .. } => Member::ControlHalt,
            R::ControlResume { .. } => Member::ControlResume,
            R::ControlState => Member::ControlState,
            R::ControlJournal(_) => Member::ControlJournal,
            R::ControlTerminalGrants => Member::ControlTerminalGrants,
            R::ControlTerminalRevoke(_) => Member::ControlTerminalRevoke,
            R::ControlStandingGrants => Member::ControlStandingGrants,
            R::ControlStandingRevoke(_) => Member::ControlStandingRevoke,
        }
    }
}
