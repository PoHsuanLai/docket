//! How a call ended and what the driver tracks on the way: the data and the small pure facts
//! the audit and the breaker are built from.

use crate::call::{CallEvent, CallState};
use crate::prepared::Prepared;
use action_review::{DecisionMark, DenialMark, DeniedBy, ReviewVerdict};
use docket_core::{
    AskReason, CallEnd, CallRefusal, ConfirmId, DecidedBy, Outcome, PolicyId, Preview, ReasonCode,
    ReviewError, Ruling, Stage, StepEnd, UndoId,
};
use porter_core::ModelId;

/// How a call ended, with what the audit and the caller need of it.
#[derive(Debug, Clone)]
pub(crate) struct Driven {
    pub end: CallEnd,
    pub outcome: Option<Outcome>,
    pub decided: DecidedBy,
    pub denied_by: Option<DeniedBy>,
    pub undo: Option<UndoId>,
}

/// What the driver has learned about the call so far.
pub(crate) struct Run {
    pub state: CallState,
    pub verdicts: Vec<(Stage, Result<ReviewVerdict, ReviewError>)>,
    pub codes: Vec<(Stage, ReasonCode)>,
    pub preview: Option<Preview>,
    pub receipt: Option<prov::ConfirmReceipt>,
    pub outcome: Option<Outcome>,
    pub denied_by: Option<DeniedBy>,
    pub undo: Option<UndoId>,
    pub confirm: Option<ConfirmId>,
}

impl Run {
    /// A call that has just arrived.
    pub(crate) fn new() -> Self {
        Self {
            state: CallState::Received,
            verdicts: vec![],
            codes: vec![],
            preview: None,
            receipt: None,
            outcome: None,
            denied_by: None,
            undo: None,
            confirm: None,
        }
    }

    /// Who decided the call: the person if they answered, else the last reviewer that ran, else
    /// policy alone.
    pub(crate) fn decided_by(&self, p: &Prepared) -> DecidedBy {
        match (&self.receipt, self.codes.last()) {
            (Some(receipt), _) => DecidedBy::User(receipt.clone()),
            (None, Some((stage, code))) => DecidedBy::Reviewer {
                code: *code,
                stage: *stage,
            },
            (None, None) => DecidedBy::Policy(ruling_ids(&p.ruling)),
        }
    }
}

/// What the driver does after carrying out an effect.
pub(crate) enum Next {
    Event(CallEvent),
    Stop,
}

fn ruling_ids(ruling: &Ruling) -> Vec<PolicyId> {
    match ruling {
        Ruling::Deny(ids) | Ruling::AllowJudged(ids) | Ruling::AllowFinal(ids) => ids.clone(),
        Ruling::Ask(reasons) => reasons
            .iter()
            .filter_map(|r| match r {
                AskReason::Rule(id) => Some(id.clone()),
                _ => None,
            })
            .collect(),
    }
}

pub(crate) fn code_of(verdict: &Result<ReviewVerdict, ReviewError>) -> ReasonCode {
    match verdict {
        Ok(ReviewVerdict::Allow) => ReasonCode::WithinRequest,
        Ok(ReviewVerdict::Ask { why } | ReviewVerdict::Deny { why }) => why.code,
        Err(_) => ReasonCode::ReviewerFailed,
    }
}

pub(crate) fn unissued() -> ConfirmId {
    ConfirmId::parse("c-0").expect("`c-0` is a valid confirmation id")
}

pub(crate) fn reviewer_model(stage: Stage) -> ModelId {
    let name = match stage {
        Stage::Quick => "reviewer-quick",
        Stage::Deliberate => "reviewer-deliberate",
        Stage::SecondOpinion => "reviewer-second",
    };
    ModelId::parse(name).expect("a reviewer name is a valid model id")
}

/// The breaker's mark for a call that ended: allowed if it ran, denied if policy, a reviewer or
/// the person said no, and nothing for a halt, a budget or an app that failed.
pub(crate) fn decision_of(
    driven: &Driven,
    p: &Prepared,
    at: prov::UnixSeconds,
) -> Option<DecisionMark> {
    match (&driven.end, driven.denied_by) {
        (CallEnd::Done, _) => Some(DecisionMark::Allowed),
        (CallEnd::Refused(CallRefusal::Denied(_) | CallRefusal::Unconfirmed(_)), Some(by)) => {
            Some(DecisionMark::Denied(DenialMark {
                at,
                goal: p.goal.clone(),
                args: p.digest,
                by,
            }))
        }
        (CallEnd::Refused(_), _) => None,
    }
}

/// How a call's end reads in the session history.
pub(crate) fn step_end(driven: &Driven, outcome: Option<&Outcome>) -> StepEnd {
    match &driven.end {
        CallEnd::Done => StepEnd::Done {
            said: outcome.and_then(|o| o.said.clone()),
            value: outcome
                .and_then(|o| o.value.as_ref())
                .map(|v| match &v.value {
                    docket_core::Value::Handle(h) => docket_core::Reveal::Handle(*h),
                    other => docket_core::Reveal::Plain(other.clone()),
                }),
            undo: driven.undo,
        },
        CallEnd::Refused(CallRefusal::Unconfirmed(end)) => StepEnd::Unconfirmed(*end),
        CallEnd::Refused(refusal) => StepEnd::Refused(refusal.clone()),
    }
}
