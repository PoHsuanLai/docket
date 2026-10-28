//! The routine desk's decisions: what the person filled in, as the routine the book would make.

use crate::refusal::Refusal;
use almanac_core::KindPattern;
use companion_model::helper::GoalText;
use companion_model::routine::{EventMatch, JobRef, NewRoutine, RunBudget, Schedule, Trigger};
use companion_wire::{EventForm, RoutineForm, WhenForm};
use porter_core::Count;

fn event_of(form: EventForm) -> Result<EventMatch, Refusal> {
    let kind = KindPattern::parse(&form.kind).map_err(|_| Refusal::WatchKind)?;
    let words = form.words.trim();
    if words.is_empty() {
        return Err(Refusal::WatchWords);
    }
    Ok(EventMatch {
        kind,
        space: form.space,
        words: words.to_owned(),
    })
}

fn trigger_of(when: WhenForm) -> Result<Trigger, Refusal> {
    match when {
        WhenForm::At(text) => Schedule::parse(&text)
            .map(Trigger::At)
            .map_err(Refusal::Schedule),
        WhenForm::On(event) => event_of(event).map(Trigger::On),
        WhenForm::AfterJob(job) => Ok(Trigger::AfterJob(JobRef(job))),
    }
}

/// The routine the person's form asks for, for a helper that exists.
pub fn new_routine(form: RoutineForm) -> Result<NewRoutine, Refusal> {
    let goal = GoalText::parse(&form.goal).map_err(|_| Refusal::NoGoal)?;
    let trigger = trigger_of(form.when)?;
    Ok(NewRoutine {
        helper: form.helper,
        goal,
        trigger,
        budget: RunBudget {
            steps: Count(form.steps.max(1)),
        },
        missed: form.missed,
    })
}
