//! The plan card: one step per call the task makes, and the answer phase a call's progress
//! implies. Pure; `drive.rs` feeds it what the router says and publishes the result.
//!
//! A step is `Pending` from the call's start until the router hands it to the app (`Dispatched`),
//! `Running` after, then `Done` or `Failed`. While the router waits for the person's sheet the
//! answer is `NeedsYou(Confirm)`; when the sheet closes it is `Streaming` again.

use companion_wire::{AnswerPhase, NeedsYou, PlanStepWire, PlanWire, StepWireState};
use docket_core::{ActionRef, CallId, CallProgress, CallRefusal, LabelText, StepEnd, StepId};
use prov::Effect;

/// The steps of one turn, in the order the calls were made.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    steps: Vec<PlanStepWire>,
}

impl Plan {
    /// A call has started.
    pub fn begin(&mut self, call: CallId, action: ActionRef, label: LabelText, effect: Effect) {
        let id = StepId(u32::try_from(self.steps.len()).unwrap_or(u32::MAX));
        self.steps.push(PlanStepWire {
            id,
            action,
            label,
            effect,
            state: StepWireState::Pending,
            call: Some(call),
        });
    }

    /// The router says how far a call is.
    pub fn progress(&mut self, call: CallId, progress: &CallProgress) {
        let running = matches!(
            progress,
            CallProgress::Dispatched | CallProgress::Running(_)
        );
        if running {
            self.set(call, StepWireState::Running);
        }
    }

    /// A call ended.
    pub fn end(&mut self, call: CallId, end: &StepEnd) {
        let state = match end {
            StepEnd::Done { undo, .. } => StepWireState::Done { undo: *undo },
            StepEnd::Refused(refusal) => StepWireState::Failed(refusal.clone()),
            StepEnd::Unconfirmed(why) => StepWireState::Failed(CallRefusal::Unconfirmed(*why)),
        };
        self.set(call, state);
    }

    fn set(&mut self, call: CallId, state: StepWireState) {
        if let Some(step) = self.steps.iter_mut().find(|s| s.call == Some(call)) {
            step.state = state;
        }
    }

    /// The card, once there is a step.
    pub fn wire(&self) -> Option<PlanWire> {
        (!self.steps.is_empty()).then(|| PlanWire {
            steps: self.steps.clone(),
        })
    }
}

/// The phase a call's progress puts the answer in.
pub fn phase_of(progress: &CallProgress) -> AnswerPhase {
    match progress {
        CallProgress::Confirming(id) => AnswerPhase::NeedsYou(NeedsYou::Confirm(id.clone())),
        CallProgress::Reviewing
        | CallProgress::Previewing
        | CallProgress::Dispatched
        | CallProgress::Running(_) => AnswerPhase::Streaming,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docket_core::ConfirmId;
    use prov::ActionName;

    fn action() -> ActionRef {
        ActionRef {
            app: porter_core::AppName::parse("org.quire.Mail").expect("app"),
            name: ActionName::parse("mail.message.forward").expect("name"),
        }
    }

    fn plan_with_one() -> Plan {
        let mut plan = Plan::default();
        plan.begin(
            CallId(0),
            action(),
            LabelText::parse("Forward").expect("label"),
            Effect::Outbound,
        );
        plan
    }

    fn state(plan: &Plan) -> StepWireState {
        plan.wire().expect("card").steps[0].state.clone()
    }

    #[test]
    fn no_step_no_card() {
        assert_eq!(Plan::default().wire(), None);
    }

    #[test]
    fn a_step_goes_pending_running_done() {
        let mut plan = plan_with_one();
        assert_eq!(state(&plan), StepWireState::Pending);
        plan.progress(CallId(0), &CallProgress::Reviewing);
        plan.progress(
            CallId(0),
            &CallProgress::Confirming(ConfirmId::parse("c-1").expect("id")),
        );
        assert_eq!(state(&plan), StepWireState::Pending);
        plan.progress(CallId(0), &CallProgress::Dispatched);
        assert_eq!(state(&plan), StepWireState::Running);
        plan.end(
            CallId(0),
            &StepEnd::Done {
                said: None,
                value: None,
                undo: None,
            },
        );
        assert_eq!(state(&plan), StepWireState::Done { undo: None });
    }

    #[test]
    fn a_refusal_fails_the_step() {
        let mut plan = plan_with_one();
        plan.end(CallId(0), &StepEnd::Refused(CallRefusal::Timeout));
        assert_eq!(state(&plan), StepWireState::Failed(CallRefusal::Timeout));
    }

    #[test]
    fn the_sheet_is_needs_you_and_closing_it_streams_again() {
        let id = ConfirmId::parse("c-1").expect("id");
        let table = [
            (CallProgress::Reviewing, AnswerPhase::Streaming),
            (CallProgress::Previewing, AnswerPhase::Streaming),
            (
                CallProgress::Confirming(id.clone()),
                AnswerPhase::NeedsYou(NeedsYou::Confirm(id)),
            ),
            (CallProgress::Dispatched, AnswerPhase::Streaming),
        ];
        for (progress, phase) in table {
            assert_eq!(phase_of(&progress), phase, "{progress:?}");
        }
    }
}
