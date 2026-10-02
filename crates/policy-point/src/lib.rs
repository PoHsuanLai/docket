//! The policy point: a Cedar wrapper. It holds the entity schema and the default policies, the
//! shape of a policy request, and `Pdp::decide`, which maps three Cedar queries onto the four
//! [`Ruling`](docket_core::Ruling)s. Pure: no I/O, no runtime.
//!
//! The rulings and the reasons to ask live in `docket-core` because the router's refusals and
//! the audit name them; the reviewer's `tighten` consumes a `Ruling` and may not depend on this
//! crate.

mod eval;
mod pdp;
mod request;

pub use pdp::{DEFAULT_POLICIES, Pdp, PolicyError, SCHEMA};
pub use request::{
    ActionFacts, CoverageState, GrantState, Op, PolicyContext, PolicyRequest, PrincipalFacts,
    SpaceRelation,
};
