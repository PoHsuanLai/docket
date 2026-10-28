//! The pure steps of a call's way through the router. The facts the policy reads (`facts`) and
//! the gate's mapping from a ruling (`gate`), the call lifecycle (`call`), the classification of
//! an action that classifies per call (`classify`), standing consent and the turn-start claim
//! (`consent`, `turn_start`), the checks on a call's arguments (`argcheck`), the call as the
//! router prepared it (`prepared`) and as it was driven (`driven`), the confirmation sheet's
//! facts (`confirm`, `brief_sheet`), what standing grants say of a call (`standing_ctx`), the
//! terminal's grants (`terminal`), limits (`limits`), messages (`messaging`), the companion's
//! action names (`companion_names`) and the trace spans (`spans`).
//!
//! Pure: no clock, bus, file or runtime is reached here. It names nothing of `Router` or
//! `Seams`; `docket-router` runs these steps over its seams.

pub mod argcheck;
pub mod brief_sheet;
pub mod call;
pub mod classify;
pub mod companion_names;
pub mod confirm;
pub mod consent;
pub mod coverage;
pub mod driven;
pub mod facts;
pub mod gate;
pub mod limits;
pub mod may_make;
pub mod messaging;
pub mod policylog;
pub mod prepared;
pub mod spans;
pub mod standing_ctx;
mod standing_facts;
pub mod terminal;
pub mod turn_start;
