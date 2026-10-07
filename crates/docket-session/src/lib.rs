//! The durable session, portable and pure (design note `acp-sessions.md` sections 4 and 5).
//!
//! - `SessionEntry`: one thing that happened to a session. A session's log is its entries, in
//!   order, appended and never changed; `encode` and `decode` are the stored form, with an
//!   explicit version, under the kind tags `companion.session.<slug>`.
//! - `resume_plan`: entries in, a `ResumePlan` out. Taint never goes down, a call that was
//!   in flight ends interrupted and is never run again, the policy comes back as stored, handles
//!   come back as labels, and a gap or an unreadable entry fails closed.
//! - `fork` and `export`: a child's first entries, and a stable JSON document.
//! - `legacy`: the records companiond wrote before (`companion_wire::SessionRecord`) read as
//!   entries.
//! - `may_restore`: who may bring a stored session back by naming it (the opener rule).
//! - `SessionLog`, `SessionBackend` and `SessionHost`: the seams. The native and ACP backends
//!   and the host are later lanes; `fake` holds a scripted backend and an in-memory log for
//!   tests.
//!
//! Nothing here reaches a bus, a runtime or a clock: every time and every id is passed in.

mod backend;
mod codec;
mod entry;
mod export;
pub mod fake;
pub mod fake_host;
mod fork;
pub mod legacy;
mod log;
mod plan;
mod restore_rule;
mod resume;

pub use backend::{
    BackendEvent, BackendFault, CallEvent, HostFault, Resumed, SessionBackend, SessionHost,
    StartSession, TurnEnd, UsageNote,
};
pub use codec::{
    CURRENT, EncodeFault, Encoded, EntryVersion, Logged, Read, Unreadable, decode, encode, kind_tag,
};
pub use entry::{
    BackendKind, BreakerNote, CallOpen, EndCause, ForkPoint, HandleLabel, Opening, ProgramName,
    ProgramNameError, Seq, SessionEntry, SkillUse, Taint, TaintCause, TaintNote, Workspace,
    WorkspaceError,
};
pub use export::{EXPORT_VERSION, ExportFault, SessionExport, export, from_json, to_json};
pub use fork::{ForkFault, fork};
pub use log::{Appended, LogFault, LogPage, PageSize, SessionLog, read_all};
pub use plan::{
    Blocker, Interrupted, PlanRefusal, ResumeFault, ResumePlan, ResumedBudget, Standing,
};
pub use restore_rule::{Claimant, may_restore};
pub use resume::resume_plan;
