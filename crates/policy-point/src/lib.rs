//! The policy point: a Cedar wrapper. It holds the entity schema and the built-in policies (the
//! core and the two presets), the shape of a policy request, `Pdp::layered`, which builds one
//! set from the layers (core, preset, person, assistant, administrator) and decides what to do
//! with a source that is bad, and `Pdp::decide`, which maps three Cedar queries onto the four
//! [`Ruling`](docket_core::Ruling)s. Pure: no I/O, no runtime; the caller reads the files and
//! passes their text as [`PolicySource`]s.
//!
//! The rulings and the reasons to ask live in `docket-core` because the router's refusals and
//! the audit name them; the reviewer's `tighten` consumes a `Ruling` and may not depend on this
//! crate.

mod eval;
mod fault;
mod layered;
mod layers;
mod pdp;
mod presets;
mod request;

pub use fault::{EvalFault, FaultCause, Ruled};
pub use layered::{LayerError, Layered};
pub use layers::{Layer, PolicyOrigin, PolicySource, Rejected};
pub use pdp::{AUTO_PRESET, CORE_POLICIES, MANUAL_PRESET, Pdp, PolicyError, SCHEMA, preset_text};
pub use presets::Presets;
pub use request::{
    ActionFacts, BudgetFact, CoverageState, GrantState, HaltFact, LimitFact, Op, PolicyContext,
    PolicyRequest, PrincipalFacts, RepeatFact, RunFact, SpaceRelation, TaintFact, TerminalGrant,
};
