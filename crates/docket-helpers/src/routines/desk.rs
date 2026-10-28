//! The routines as the desk the shell talks to: the list, a routine's card with its runs, and
//! making, pausing, resuming and removing one. Every refusal is a typed `Refusal`, told in plain
//! words at this edge.

use super::host::Host;
use super::scheduler::Routines;
use crate::words::sentence;
use companion_core::{Refusal, new_routine};
use companion_wire::{
    RoutineCardView, RoutineForm, RoutineLine, RoutineReply, RoutinesView, routine_card,
    routine_line,
};
use docket_core::helper::HelperName;
use docket_core::routine::{Routine, RoutineEvent, RoutineId};
use docket_tasks::Pending;
use docket_vocab::Clock;
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

impl<H: Host, K: Clock> Routines<H, K> {
    fn name_in_line(&self, routine: &Routine) -> HelperName {
        self.host()
            .name_of(&routine.helper)
            .unwrap_or_else(|| HelperName::parse("A helper that is gone").expect("a name"))
    }

    async fn make(&self, form: RoutineForm) -> RoutineReply {
        match self.try_make(form).await {
            Ok(id) => RoutineReply::Made(id),
            Err(why) => RoutineReply::Refused(sentence(&why)),
        }
    }

    async fn try_make(&self, form: RoutineForm) -> Result<RoutineId, Refusal> {
        if self.host().space_of(&form.helper).is_none() {
            return Err(Refusal::UnknownHelper);
        }
        let new = new_routine(form)?;
        let now = self.now();
        self.apply(RoutineEvent::Add {
            new: Box::new(new),
            now,
            zone: self.host().zone(now),
        })
        .await
        .ok_or(Refusal::RoutineNotMade)
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
