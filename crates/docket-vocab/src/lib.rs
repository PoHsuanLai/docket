//! docket's smallest vocabulary, the bottom of the stack: ids and units, values and the
//! arguments of a call, the call request and its class, RFC 3339 dates and instants, the
//! workspace name, the text marks a name may not carry and the `Clock` seam.
//!
//! Pure and portable: serde, `chrono` for dates, `porter-core` for app names and `prov` for who
//! and what. Nothing here reads a clock, a file or the environment.

mod classify;
mod clock;
mod ids;
mod marks;
mod request;
mod units;
mod value;
pub mod when;
mod workspace;

pub use classify::{
    CallClass, Classification, ClassifyAnswer, ClassifyFault, DelegationEnd, PerCall,
};
pub use clock::Clock;
pub use ids::{
    ActionRef, CallId, CardActionId, ChoiceId, EntityRef, FileRef, Handle, IconName, IntentsVocab,
    LabelText, NoteSlug, NoteSlugError, ParamName, RelationName, StepId, TextTargetRef, TurnId,
    UndoId, UndoToken, UtteranceId, ViewName, WindowKey, action_prefix,
};
pub use marks::{hides, plain_text, reorders};
pub use request::{CallRequest, Origin, RECALL_ACTION, RECALL_APP};
pub use units::{CharCount, Days, Depth, Generation, Millis, Scale, Seconds};
pub use value::{Args, ChoiceDecl, CivilDate, Decimal, Lines, ParamType, TargetValue, Value};
pub use workspace::{Workspace, WorkspaceError};
