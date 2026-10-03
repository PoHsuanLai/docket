//! The auto-mode reviewer, which only tightens.
//!
//! Cedar rules first; a reviewer cascade may then only move an `AllowJudged` ruling toward
//! `Ask` or `Deny`, never loosen anything Cedar asked or denied. The reviewer's input is
//! stripped to the person's turns, the typed action and labels: no tool output, no agent prose,
//! no quoted untrusted content. A breaker stops a session after repeated denials.
//!
//! Pure over its seams: the model-backed reviewer is generic over porter-infer's `Model`.

mod breaker;
mod cascade;
mod infer;
mod parse;
mod render;
mod request;
mod verdict;

pub use breaker::{
    ArgDigest, Breaker, DecisionMark, DenialMark, DeniedBy, GoalKey, RepeatState, note, repeated,
};
pub use cascade::{
    Gate, ModelChoice, ModelFamily, Reviewer, ReviewerSet, SetFault, escalate, plan, tighten,
};
pub use infer::{InferReviewer, REVIEW_CLASS};
pub use parse::parse_verdict;
pub use render::render;
pub use request::{
    ArgView, CallEndKind, ProposedAction, ReviewPrompt, ReviewRequest, TypedStep, verdict_shape,
};
pub use verdict::{ReviewReason, ReviewVerdict};
