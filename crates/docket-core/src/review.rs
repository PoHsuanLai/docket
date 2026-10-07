//! The vocabulary of gating that the wire, the audit and every crate above share: the rulings
//! of policy, the reasons to ask, the stages of review, and the breaker's trips. The logic that
//! produces them lives in `policy-point` and `action-review`; the words live here because the
//! refusals and audit records of this crate name them.

use crate::units::Millis;
use porter_core::{Count, ModelId};
use prov::Effect;
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::manifest::ArgSink;

/// How much the router asks, per Space (setting `agent.strictness`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strictness {
    /// Review more, ask more.
    AskMore,
    /// The settled default (COMPANION item 9).
    Default,
    /// Let two reviewers of different families decide outbound and destructive acts inside a
    /// task policy.
    TrustMore,
}

/// The id of one Cedar policy (`@id("rule-of-two")`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PolicyId(pub String);

/// Why the router asks the person.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum AskReason {
    /// The effect class itself (outbound, destructive).
    Effect(Effect),
    /// The planner read untrusted content.
    Tainted,
    /// The target is in another Space.
    CrossSpace,
    /// More things than the mass threshold.
    Mass(Count),
    /// First use of this app, class and Space.
    FirstUse,
    /// The call came from the terminal (`quire-do`): a terminal cannot tell the person from an
    /// agent typing in it, so the person is asked unless they gave a standing grant.
    FromTerminal,
    /// The action asks every time.
    AskAlways,
    /// Lasting memory from untrusted input.
    LastingFromUntrusted,
    /// Private data and untrusted input and an outbound channel in one session.
    RuleOfTwo,
    /// An untrusted value feeds this sink.
    UntrustedSink(ArgSink),
    /// The call is outside the task policy derived from the person's own words.
    OutsideTask,
    /// Reviewers of different families disagreed.
    Disagreement,
    /// A named Cedar rule.
    Rule(PolicyId),
}

/// What policy rules before any reviewer: four outcomes, from three Cedar queries
/// (`perform`, `perform_unasked`, `perform_unjudged`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Ruling {
    /// `perform` is not permitted: refuse. An empty list means no policy permitted it.
    Deny(Vec<PolicyId>),
    /// `perform_unasked` is not permitted: ask the person.
    Ask(Vec<AskReason>),
    /// `perform_unjudged` is not permitted: a reviewer may tighten, never loosen.
    AllowJudged(Vec<PolicyId>),
    /// All three are permitted.
    AllowFinal(Vec<PolicyId>),
}

/// The coarse reason a planner is told (no policy ids, no reviewer text: no oracle).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DenyCode {
    /// Outside what the person asked for.
    OutsideTask,
    /// Not something an agent may do.
    NotAllowed,
    /// Needs the person.
    NeedsUser,
    /// The same call was already refused.
    Repeated,
}

/// Why the breaker tripped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakerTrip {
    /// Three denials in a row (`agent.breaker.consecutive`).
    Consecutive,
    /// Ten of the last fifty (`agent.breaker.recent`).
    Recent,
    /// The same goal with different arguments, three times.
    Probing,
}

/// One stage of the review cascade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// A small local judge: one grammar-constrained token, tuned to over-flag.
    Quick,
    /// A larger local model, after a Quick flag or for high impact.
    Deliberate,
    /// A model of another family, for high-impact allows.
    SecondOpinion,
}

/// How much is at stake, which sets how many stages run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Impact {
    /// An ordinary change.
    Low,
    /// Outbound or destructive, or a count above half the mass threshold, or lasting memory.
    High,
}

/// A reviewer's coded reason (the text is for the person's activity view only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    /// Within what the person asked.
    WithinRequest,
    /// Covered by the task policy.
    CoveredByTaskPolicy,
    /// A routine act.
    Routine,
    /// Outside the request.
    OutsideRequest,
    /// More than asked for.
    ScopeCreep,
    /// Looks like exfiltration.
    Exfiltration,
    /// Cannot be taken back.
    Irreversible,
    /// Looks like an injection.
    InjectionSuspected,
    /// The reviewer could not tell.
    Uncertain,
    /// The reviewer failed.
    ReviewerFailed,
    /// Two reviewers of different families disagreed.
    Disagreement,
}

/// A reviewer's words, at most 200 characters, model-written: shown only in the person's
/// activity view, never to a planner.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReasonText(pub String);

impl fmt::Debug for ReasonText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ReasonText(<{} bytes>)", self.0.len())
    }
}

/// What kind of verdict a reviewer gave, for the audit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerdictKind {
    /// Allow.
    Allow,
    /// Ask.
    Ask,
    /// Deny.
    Deny,
}

/// Why a model call that gates (a reviewer, the policy writer) gave nothing usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum ReviewError {
    /// It took too long.
    #[error("timed out")]
    Timeout,
    /// No model could be reached.
    #[error("unavailable")]
    Unavailable,
    /// The reply did not parse.
    #[error("unparseable reply")]
    Unparseable,
    /// The model reasoned and wrote no reply: its token budget went on thought. Handled as
    /// `Unparseable` is (the person is asked); the audit and traces say what happened.
    #[error("only reasoning, no reply")]
    OnlyThought,
    /// The reply used a word outside the vocabulary.
    #[error("reply outside the vocabulary")]
    OutOfVocabulary,
}

/// How a review ended, for the audit: what ran, how long, and on which model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewMark {
    /// The stage.
    pub stage: Stage,
    /// The verdict, or why there was none.
    pub verdict: Result<VerdictKind, ReviewError>,
    /// The coded reason.
    pub code: ReasonCode,
    /// How long.
    pub latency: Millis,
    /// The model that answered.
    pub model: ModelId,
}
