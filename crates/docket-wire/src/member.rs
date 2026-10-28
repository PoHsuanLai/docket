//! Which member of `Intents1` a request is: the fieldless name of each `IntentsRequest` variant,
//! written once, with the request's `member` method and the list of every member made from it.

use crate::request::IntentsRequest;

/// Declares `Member`, `Member::ALL` and `IntentsRequest::member` from one list of
/// `Member <= request pattern` rows, in the order the interface table lists the members.
macro_rules! members {
    ($($(#[$doc:meta])* $name:ident <= $pat:pat,)*) => {
        /// One member of `org.quire.Intents1`, as a word.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Member {
            $($(#[$doc])* $name,)*
        }

        impl Member {
            /// Every member, in the order the interface table lists them.
            pub const ALL: [Member; [$(Member::$name),*].len()] = [$(Member::$name),*];
        }

        impl IntentsRequest {
            /// The member this request calls.
            pub fn member(&self) -> Member {
                use IntentsRequest as R;
                match self {
                    $($pat => Member::$name,)*
                }
            }
        }
    };
}

members! {
    /// `.Registry.Manifests`.
    Manifests <= R::Manifests,
    /// `.Index.Push`.
    IndexPush <= R::IndexPush(_),
    /// `.Index.Reset`.
    IndexReset <= R::IndexReset { .. },
    /// `.Search.Query`.
    SearchQuery <= R::Search(_),
    /// `.Search.Cancel`.
    SearchCancel <= R::SearchCancel(_),
    /// `.Run.Perform`.
    Perform <= R::Perform { .. },
    /// `.Run.DryRun`.
    DryRun <= R::DryRun { .. },
    /// `.Run.Preview`.
    Preview <= R::Preview(_),
    /// `.Run.Suggest`.
    Suggest <= R::Suggest(_),
    /// `.Run.Undo`.
    Undo <= R::Undo(_),
    /// `.Run.UndoAll`.
    UndoAll <= R::UndoAll(_),
    /// `.Context.Current`.
    Context <= R::Context { .. },
    /// `.Session.Open`.
    SessionOpen <= R::SessionOpen(_),
    /// `.Session.Turn`.
    SessionTurn <= R::SessionTurn { .. },
    /// `.Session.Close`.
    SessionClose <= R::SessionClose { .. },
    /// `.Session.TurnEnded`.
    SessionTurnEnded <= R::SessionTurnEnded { .. },
    /// `.Session.SetMode`.
    SessionMode <= R::SessionMode(_),
    /// `.Session.Resolve`.
    SessionResolve <= R::SessionResolve { .. },
    /// `.Session.Display`.
    SessionDisplay <= R::SessionDisplay { .. },
    /// `.Session.DisplayLabelled`.
    SessionDisplayLabelled <= R::SessionDisplayLabelled { .. },
    /// `.Session.Read`.
    SessionRead <= R::SessionRead { .. },
    /// `.Session.TaskPolicy`.
    SessionTaskPolicy <= R::SessionTaskPolicy { .. },
    /// `.Session.Widen`.
    SessionWiden <= R::SessionWiden { .. },
    /// `.Session.Note`.
    SessionNote <= R::SessionNote { .. },
    /// `.Session.Recall`.
    SessionRecall <= R::SessionRecall { .. },
    /// `.Session.Narrow`.
    SessionNarrow <= R::SessionNarrow { .. },
    /// `.Session.Handles`.
    SessionHandles <= R::SessionHandles { .. },
    /// `.Session.Stored`.
    SessionStored <= R::SessionStored { .. },
    /// `.Checkpoint.List`.
    CheckpointList <= R::CheckpointList { .. },
    /// `.Checkpoint.Plan`.
    CheckpointPlan <= R::CheckpointPlan { .. },
    /// `.Checkpoint.Watch`.
    CheckpointWatch <= R::CheckpointWatch { .. },
    /// `.Checkpoint.Mark`.
    CheckpointMark <= R::CheckpointMark { .. },
    /// `.Checkpoint.WatchAgent`.
    CheckpointWatchAgent <= R::CheckpointWatchAgent { .. },
    /// `.Checkpoint.Handover`.
    CheckpointHandover <= R::CheckpointHandover { .. },
    /// `.Checkpoint.Watched`.
    CheckpointWatched <= R::CheckpointWatched,
    /// `.Message.Send`.
    MessageSend <= R::MessageSend { .. },
    /// `.Message.Inbox`.
    MessageInbox <= R::MessageInbox(_),
    /// `.Gate.Grant`.
    GateGrant <= R::GateGrant(_),
    /// `.Gate.Check`.
    GateCheck <= R::GateCheck(_),
    /// `.Control.Halt`.
    ControlHalt <= R::ControlHalt { .. },
    /// `.Control.Resume`.
    ControlResume <= R::ControlResume { .. },
    /// `.Control.State`.
    ControlState <= R::ControlState,
    /// `.Control.Journal`.
    ControlJournal <= R::ControlJournal(_),
    /// `.Control.TerminalGrants`.
    ControlTerminalGrants <= R::ControlTerminalGrants,
    /// `.Control.RevokeTerminalGrant`.
    ControlTerminalRevoke <= R::ControlTerminalRevoke(_),
    /// `.Control.StandingGrants`.
    ControlStandingGrants <= R::ControlStandingGrants,
    /// `.Control.RevokeStandingGrant`.
    ControlStandingRevoke <= R::ControlStandingRevoke(_),
}
