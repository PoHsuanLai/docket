//! The routines as the desk the shell talks to: the list, a routine's card with its runs, and
//! making, pausing, resuming and removing one. Every refusal is a plain sentence.

use super::host::Host;
use super::scheduler::Routines;
use almanac_core::KindPattern;
use companion_wire::{
    EventForm, RoutineCardView, RoutineForm, RoutineLine, RoutineReply, RoutinesView, WhenForm,
    routine_card, routine_line,
};
use docket_core::Clock;
use docket_core::helper::{GoalText, HelperName};
use docket_core::routine::{
    EventMatch, JobRef, NewRoutine, Routine, RoutineEvent, RoutineId, RunBudget, Schedule, Trigger,
};
use docket_tasks::Pending;
use porter_core::Count;
use std::fmt;

/// What the shell asks of the routines.
pub trait RoutineDesk: fmt::Debug + Send + Sync {
    /// The list.
    fn view(&self) -> RoutinesView;

    /// One routine with its recent runs.
    fn card(&self, routine: RoutineId) -> Option<RoutineCardView>;

    /// Makes a routine from what the person filled in.
    fn create(&self, form: RoutineForm) -> Pending<'_, RoutineReply>;

    /// The person pauses a routine.
    fn pause(&self, routine: RoutineId) -> Pending<'_, ()>;

    /// The person resumes a routine.
    fn resume(&self, routine: RoutineId) -> Pending<'_, ()>;

    /// The person removes a routine.
    fn remove(&self, routine: RoutineId) -> Pending<'_, ()>;
}

fn event_of(form: EventForm) -> Result<EventMatch, String> {
    let kind = KindPattern::parse(&form.kind)
        .map_err(|_| "Say which kind of thing to watch for.".to_owned())?;
    let words = form.words.trim();
    if words.is_empty() {
        return Err("Say what to watch for, in a few words.".to_owned());
    }
    Ok(EventMatch {
        kind,
        space: form.space,
        words: words.to_owned(),
    })
}

fn trigger_of(when: WhenForm) -> Result<Trigger, String> {
    match when {
        WhenForm::At(text) => Schedule::parse(&text)
            .map(Trigger::At)
            .map_err(|fault| fault.to_string()),
        WhenForm::On(event) => event_of(event).map(Trigger::On),
        WhenForm::AfterJob(job) => Ok(Trigger::AfterJob(JobRef(job))),
    }
}

impl<H: Host, K: Clock> Routines<H, K> {
    fn name_in_line(&self, routine: &Routine) -> HelperName {
        self.host()
            .name_of(&routine.helper)
            .unwrap_or_else(|| HelperName::parse("A helper that is gone").expect("a name"))
    }

    async fn make(&self, form: RoutineForm) -> RoutineReply {
        if self.host().space_of(&form.helper).is_none() {
            return RoutineReply::Refused("There is no such helper.".to_owned());
        }
        let Ok(goal) = GoalText::parse(&form.goal) else {
            return RoutineReply::Refused("Say what it is to do, in a few sentences.".to_owned());
        };
        let trigger = match trigger_of(form.when) {
            Ok(trigger) => trigger,
            Err(why) => return RoutineReply::Refused(why),
        };
        let now = self.now();
        let made = self
            .apply(RoutineEvent::Add {
                new: Box::new(NewRoutine {
                    helper: form.helper,
                    goal,
                    trigger,
                    budget: RunBudget {
                        steps: Count(form.steps.max(1)),
                    },
                    missed: form.missed,
                }),
                now,
                zone: self.host().zone(now),
            })
            .await;
        match made {
            Some(id) => RoutineReply::Made(id),
            None => RoutineReply::Refused("The routine could not be made.".to_owned()),
        }
    }
}

impl<H: Host, K: Clock> RoutineDesk for Routines<H, K> {
    fn view(&self) -> RoutinesView {
        RoutinesView {
            routines: self
                .book()
                .routines()
                .iter()
                .map(|routine| routine_line(routine, self.name_in_line(routine)))
                .collect::<Vec<RoutineLine>>(),
        }
    }

    fn card(&self, routine: RoutineId) -> Option<RoutineCardView> {
        let book = self.book();
        let found = book.routine(routine)?;
        Some(routine_card(found, self.name_in_line(found)))
    }

    fn create(&self, form: RoutineForm) -> Pending<'_, RoutineReply> {
        Box::pin(self.make(form))
    }

    fn pause(&self, routine: RoutineId) -> Pending<'_, ()> {
        Box::pin(async move {
            self.apply(RoutineEvent::Pause(routine)).await;
        })
    }

    fn resume(&self, routine: RoutineId) -> Pending<'_, ()> {
        Box::pin(async move {
            let now = self.now();
            let zone = self.host().zone(now);
            self.apply(RoutineEvent::Resume {
                id: routine,
                now,
                zone,
            })
            .await;
        })
    }

    fn remove(&self, routine: RoutineId) -> Pending<'_, ()> {
        Box::pin(async move {
            self.apply(RoutineEvent::Remove(routine)).await;
        })
    }
}
