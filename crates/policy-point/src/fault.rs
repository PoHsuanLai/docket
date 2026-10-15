//! What went wrong while Cedar ruled on a request. A fault never permits: the ruling that comes
//! with it is the stricter reading of the query that faulted.

use docket_core::Ruling;

/// Why one query could not be answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FaultCause {
    /// A principal, app or action name is not a Cedar name, or the entities do not fit the schema.
    #[error("the request's entities do not fit the schema")]
    Entities,
    /// The query's action is not a Cedar name.
    #[error("the query's action is not a Cedar name")]
    Action,
    /// The context does not fit the schema.
    #[error("the request's context does not fit the schema")]
    Context,
    /// Cedar refused the request as a whole.
    #[error("Cedar refused the request")]
    Request,
    /// This many policies raised an evaluation error (a policy that errors is skipped; this
    /// includes Cedar refusing to evaluate on a stack that is too small).
    #[error("{0} policies failed to evaluate")]
    Policies(usize),
}

/// One query that failed: which, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("query `{query}`: {cause}")]
pub struct EvalFault {
    /// The Cedar action of the query (`perform`, `perform_unasked`, ...).
    pub query: &'static str,
    /// What failed.
    pub cause: FaultCause,
}

/// A ruling and the faults met reaching it. Empty `faults` is the ordinary case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ruled {
    /// The ruling, fail-safe: a faulted query counts as not permitted.
    pub ruling: Ruling,
    /// Every query that faulted, in the order asked.
    pub faults: Vec<EvalFault>,
}
