//! The call lifecycle as a pure step (call lifecycle §4.1 with the addendum's rows): the
//! router feeds it events and carries out the effects it returns. The decision before dispatch
//! is [`gate`](crate::gate); this machine sequences review, preview, confirmation and dispatch
//! around it.

use crate::gate::Pending;
use action_review::ReviewVerdict;
use docket_core::{
    AppRefusal, AuditRecord, CallEnd, CallProgress, ConfirmAnswer, ConfirmEnd, ConfirmId,
    ConfirmRequest, Outcome, Preview, ReviewError, Stage,
};

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

/// What happens to a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallEvent {
    /// The arguments checked out, or why not.
    ArgsChecked(Result<(), docket_core::ArgFault>),
    /// The gate decided.
    Gated(Pending),
    /// A stage answered or failed.
    Verdict(Stage, Result<ReviewVerdict, ReviewError>),
    /// The app described the change, or could not.
    Previewed(Box<Result<Preview, AppRefusal>>),
    /// The person answered.
    Answered(ConfirmAnswer),
    /// The app answered.
    AppAnswered(Box<Result<Outcome, AppRefusal>>),
    /// The app did not answer in time.
    AppTimedOut,
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
pub fn call_step(state: CallState, event: CallEvent) -> (CallState, Vec<CallEffect>) {
    let _ = (state, event);
    todo!(
        "call_step: the rows of call lifecycle section 4.1 with the addendum's: Gating -> Reviewing/Previewing/Dispatched/Done; Reviewing -> Dispatched only when tighten says Run; Confirming -> Dispatched on a receipt; a halt cancels the sheet and the review; an in-flight app call continues and its result is journalled"
    )
}
