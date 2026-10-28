//! The helpers' model: persistent, named agents the person keeps, the jobs they do, the routines
//! that start jobs without being asked, and the hand-over of a conversation to a watched terminal
//! agent. Pure state machines over values: the card and the board of jobs (`helper`), the routine
//! book and its schedules (`routine`) and the watched-agent shapes (`hand`). Nothing here reads a
//! clock, a bus or a model; the host owns the board and the book and carries out the effects.
//!
//! Pure and portable: serde only. Names `docket-vocab` for ids, `docket-manifest` for what an app
//! declares, `docket-permissions-core` for the per-task policy a reach is held to, `porter-core`
//! and `porter-infer` for counts and model references, `prov` for who and what, and
//! `almanac-core` for the event shapes a routine matches.

mod hand;
pub mod helper;
pub mod routine;

pub use hand::{
    AppSessionId, AppSessionIdError, HandNote, HandTo, SeenCommand, SeenCommandFault, WatchOwner,
    WatchedAgent, WatchedLine, Watching,
};
