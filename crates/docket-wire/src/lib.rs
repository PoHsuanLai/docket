//! The wire of docket's `org.quire.Intents1` and the shapes that ride on it: every request and
//! reply, the bodies they carry, who is calling (`CallerId`, over the roles of `docket-vocab`) and which member a
//! request is (`Member`), where a conversation was started from, the opening of a session and the
//! ledger of a task, and summoning the companion into an app. Nothing here sends or receives: a
//! transport carries these values as JSON.
//!
//! Pure and portable: serde only. Sits above `docket-core` (the vocabulary the bodies hold) and
//! `docket-gate-wire` (the computer-use gate's ask and answer).

mod bodies;
mod caller;
mod member;
mod reply;
mod request;
mod started;
mod summon;
mod task;

pub use bodies::{
    Displayed, Envelope, GrantAnswer, GrantAsk, HelperNote, JournalFilter, NoteAsk, ReadAsk,
    RecallAsk, RecallView, RecentLine, Resolved, SessionModeAsk, SessionNote, StoredAsk, StoredRow,
    StoredView, UndoReport, WidenAnswer, WidenAsk, WireRefusal,
};
pub use caller::{CallerId, is_terminal_scope};
pub use member::Member;
pub use reply::IntentsReply;
pub use request::IntentsRequest;
pub use started::{NotATerminalScope, StartedFrom, TerminalScope};
pub use summon::{SummonAnswer, SummonOrigin, SummonSerial, VoiceIntent};
pub use task::{
    LedgerStep, SessionOpen, SessionOpened, TaskKind, TaskLedger, TaskStart, close, skeleton_of,
    step_outcome,
};
