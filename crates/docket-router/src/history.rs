//! Earlier turns read as history, not as what works now. A step that failed in an earlier
//! turn is a fact about that moment (an app was not ready, a model was not there); handed to
//! a planner as recalled text it reads as a standing fact, and a new request gives up before
//! trying. The planner's recall leaves those step lines out. Memory itself keeps them, so what
//! the person sees and recalls themselves is unchanged.

use almanac_core::{Skeleton, StepOutcome};

/// The end of a skeleton text line for a step that failed (`step: <action> <targets> failed`).
const FAILED_STEP_END: &str = " failed";
/// The start of a skeleton text line for a step.
const STEP_START: &str = "step: ";

/// `skeleton` without its failed steps.
pub(crate) fn without_failed_steps(skeleton: &Skeleton) -> Skeleton {
    Skeleton {
        steps: skeleton
            .steps
            .iter()
            .filter(|s| s.outcome != StepOutcome::Failed)
            .cloned()
            .collect(),
        ..skeleton.clone()
    }
}

/// Recalled skeleton `text` without the lines of failed steps; every other line stays.
pub(crate) fn without_failed_lines(text: &str) -> String {
    text.lines()
        .filter(|l| !(l.starts_with(STEP_START) && l.ends_with(FAILED_STEP_END)))
        .collect::<Vec<_>>()
        .join("\n")
}
