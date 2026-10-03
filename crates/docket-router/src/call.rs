//! The call lifecycle as a pure step (call lifecycle §4.1 with the addendum's rows): the
//! router feeds it events and carries out the effects it returns. The decision before dispatch
//! is [`gate`](crate::gate); this machine sequences review, preview, confirmation and dispatch
//! around it.

use crate::gate::Pending;
use action_review::{Gate, ReviewVerdict, escalate, tighten};
use docket_core::{
    AppRefusal, AuditRecord, CallEnd, CallProgress, CallRefusal, ConfirmAnswer, ConfirmEnd,
    ConfirmId, ConfirmRequest, GrantScope, Outcome, ParamName, ReviewError, Ruling, Stage,
    Undoable,
};
use porter_core::AppName;
use prov::SpaceScope;

/// Where a call is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallState {
    /// Arrived, arguments not yet checked.
    Received,
    /// Being gated.
    Gating,
    /// A reviewer may tighten it.
    Reviewing {
        /// The stages planned.
        planned: Vec<Stage>,
        /// The verdicts so far.
        done: Vec<(Stage, Result<ReviewVerdict, ReviewError>)>,
    },
    /// Asking the app to describe the change (a failure falls back to the plain argument lines).
    Previewing,
    /// Waiting for the person.
    Confirming(ConfirmId),
    /// Handed to the app.
    Dispatched,
    /// Over.
    Done(CallEnd),
}

/// The parameter an argument check refused, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgsRefused {
    /// Which parameter.
    pub param: ParamName,
    /// Why.
    pub why: docket_core::ArgFault,
}

/// What happens to a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallEvent {
    /// The arguments checked out, or which parameter was refused and why.
    ArgsChecked(Result<(), ArgsRefused>),
    /// The gate decided.
    Gated(Pending),
    /// A stage answered or failed.
    Verdict(Stage, Result<ReviewVerdict, ReviewError>),
    /// The router asked the app to describe the change (a failure falls back to the argument
    /// lines) and built the sheet from the answer: this is what the person will be shown.
    Previewed(Box<ConfirmRequest>),
    /// The person answered.
    Answered(ConfirmAnswer),
    /// The app answered.
    AppAnswered(Box<Result<Outcome, AppRefusal>>),
    /// The app did not answer in time.
    AppTimedOut,
    /// The app is not there (`CallRefusal::AppUnavailable`).
    AppUnavailable(AppName),
    /// A halt arrived.
    Halted,
}

/// What the router does about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallEffect {
    /// Append to the log.
    Audit(Box<AuditRecord>),
    /// Tell the requester how far it is.
    Progress(CallProgress),
    /// Run this stage.
    StartReview(Stage),
    /// Ask the app for a dry run.
    DryRun,
    /// Draw this on the sheet.
    Confirm(Box<ConfirmRequest>),
    /// Withdraw the sheet.
    CancelConfirm,
    /// Remember an `Always` the person gave.
    RecordGrant,
    /// Remember "allow from the terminal until logout".
    RecordTerminalGrant,
    /// Charge the ledger and call the app.
    Dispatch,
    /// Count a denial against the breaker.
    NoteDenial,
    /// Journal the undo token.
    Journal,
    /// The person did not confirm.
    Unconfirmed(ConfirmEnd),
}

/// One transition.
///
/// The step holds no data about the call beyond its state. The sheet arrives built in
/// `Previewed`, and the step emits it as `Confirm`; the router writes the audit record when the
/// state becomes `Done`. Everything else the step decides is here.
pub fn call_step(state: CallState, event: CallEvent) -> (CallState, Vec<CallEffect>) {
    use CallEvent as V;
    use CallState as S;
    match (state, event) {
        (done @ S::Done(_), _) => (done, vec![]),
        (S::Received, V::ArgsChecked(Err(ArgsRefused { param, why }))) => {
            (refused(CallRefusal::BadArgs { param, why }), vec![])
        }
        (S::Received, V::ArgsChecked(Ok(()))) => (S::Gating, vec![]),
        (S::Received | S::Gating | S::Reviewing { .. } | S::Previewing, V::Halted) => {
            (halted(), vec![])
        }
        (S::Gating, V::Gated(pending)) => gated(pending),
        (S::Reviewing { planned, mut done }, V::Verdict(stage, result)) => {
            done.push((stage, result));
            reviewed(planned, done)
        }
        (S::Previewing, V::Previewed(request)) => {
            let id = request.id.clone();
            (
                S::Confirming(id.clone()),
                vec![
                    CallEffect::Progress(CallProgress::Confirming(id)),
                    CallEffect::Confirm(request),
                ],
            )
        }
        (S::Confirming(_), V::Answered(ConfirmAnswer::Allowed { scope, .. })) => {
            let grant = (scope == GrantScope::Always).then_some(CallEffect::RecordGrant);
            let dispatch = [
                CallEffect::Progress(CallProgress::Dispatched),
                CallEffect::Dispatch,
            ];
            (S::Dispatched, grant.into_iter().chain(dispatch).collect())
        }
        (S::Confirming(_), V::Answered(ConfirmAnswer::AllowedFromTerminal { .. })) => (
            S::Dispatched,
            vec![
                CallEffect::RecordTerminalGrant,
                CallEffect::Progress(CallProgress::Dispatched),
                CallEffect::Dispatch,
            ],
        ),
        (S::Confirming(_), V::Answered(ConfirmAnswer::Ended(end))) => (
            refused(CallRefusal::Unconfirmed(end)),
            vec![CallEffect::NoteDenial, CallEffect::Unconfirmed(end)],
        ),
        (S::Confirming(_), V::Halted) => (halted(), vec![CallEffect::CancelConfirm]),
        (S::Dispatched, V::AppAnswered(answer)) => match *answer {
            Ok(outcome) => {
                let journal =
                    matches!(outcome.undo, Undoable::Yes(_)).then_some(CallEffect::Journal);
                (
                    CallState::Done(CallEnd::Done),
                    journal.into_iter().collect(),
                )
            }
            Err(refusal) => (refused(CallRefusal::App(refusal)), vec![]),
        },
        (S::Dispatched, V::AppTimedOut) => (refused(CallRefusal::Timeout), vec![]),
        (S::Dispatched, V::AppUnavailable(app)) => {
            (refused(CallRefusal::AppUnavailable(app)), vec![])
        }
        // An app call in flight cannot be recalled: its result is still journalled.
        (state, _) => (state, vec![]),
    }
}

fn refused(why: CallRefusal) -> CallState {
    CallState::Done(CallEnd::Refused(why))
}

/// A halt does not know its Space: the router narrows the scope when it records the end.
fn halted() -> CallState {
    refused(CallRefusal::Halted(SpaceScope::Any))
}

fn gated(pending: Pending) -> (CallState, Vec<CallEffect>) {
    match pending {
        Pending::Run => dispatched(),
        Pending::NeedsReview(planned) => match planned.first().copied() {
            Some(first) => (
                CallState::Reviewing {
                    planned,
                    done: vec![],
                },
                vec![
                    CallEffect::Progress(CallProgress::Reviewing),
                    CallEffect::StartReview(first),
                ],
            ),
            None => previewing(),
        },
        Pending::Confirm(_) => previewing(),
        Pending::Refuse(why) => {
            let denial = matches!(why, CallRefusal::Denied(_)).then_some(CallEffect::NoteDenial);
            (refused(why), denial.into_iter().collect())
        }
    }
}

/// The stages that have not answered, in plan order, or the call's fate when all have.
fn reviewed(
    planned: Vec<Stage>,
    done: Vec<(Stage, Result<ReviewVerdict, ReviewError>)>,
) -> (CallState, Vec<CallEffect>) {
    let planned = match done.first() {
        Some((Stage::Quick, quick)) if done.len() == 1 => escalate(&planned, quick),
        _ => planned,
    };
    let denied = done
        .iter()
        .any(|(_, v)| matches!(v, Ok(ReviewVerdict::Deny { .. })));
    let next = planned
        .iter()
        .find(|s| !done.iter().any(|(d, _)| d == *s))
        .copied();
    match next {
        Some(stage) if !denied => (
            CallState::Reviewing { planned, done },
            vec![CallEffect::StartReview(stage)],
        ),
        _ => match tighten(&Ruling::AllowJudged(vec![]), &planned, &done) {
            Gate::Run => dispatched(),
            Gate::Confirm => previewing(),
            Gate::Refuse(code) => (
                refused(CallRefusal::Denied(code)),
                vec![CallEffect::NoteDenial],
            ),
        },
    }
}

fn dispatched() -> (CallState, Vec<CallEffect>) {
    (
        CallState::Dispatched,
        vec![
            CallEffect::Progress(CallProgress::Dispatched),
            CallEffect::Dispatch,
        ],
    )
}

fn previewing() -> (CallState, Vec<CallEffect>) {
    (
        CallState::Previewing,
        vec![
            CallEffect::Progress(CallProgress::Previewing),
            CallEffect::DryRun,
        ],
    )
}
