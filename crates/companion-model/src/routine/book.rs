//! The book of routines, moved by one pure function. companiond owns it and carries out the
//! effects `step` returns: it writes the records, posts the run as an ordinary job on the helper,
//! tells the person what is worth telling, and feeds back what happened. Time and the zone are
//! passed in, so a test moves them by hand.

use super::kept::{Missed, Routine, RoutineId, RoutineState, RunOutcome, RunRecord};
use super::schedule::{Schedule, Zone};
use super::shapes::{JobEnd, NewRoutine, RoutineNotice, RoutineRecord, RunRequest};
use super::trigger::{EventMatch, JobRef, Trigger};
use crate::helper::{JobId, PauseWhy};
use almanac_core::EventRef;
use prov::{HelperId, UnixSeconds};
use serde::{Deserialize, Serialize};

/// How many runs in a row that do not finish pause a routine.
const IN_A_ROW: u8 = 3;
/// How late a due time may be and still run, in seconds. Past it the computer was asleep (or the
/// assistant was not running) when the time came, and the time is missed.
pub const LATE: i64 = 300;

/// Something happened to the routines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum RoutineEvent {
    /// The person made a routine.
    Add {
        /// What they made.
        new: Box<NewRoutine>,
        /// Now.
        now: UnixSeconds,
        /// The local clock.
        zone: Zone,
    },
    /// The person paused a routine.
    Pause(RoutineId),
    /// The person resumed a routine.
    Resume {
        /// Which.
        id: RoutineId,
        /// Now.
        now: UnixSeconds,
        /// The local clock.
        zone: Zone,
    },
    /// The person removed a routine.
    Remove(RoutineId),
    /// Time has come to `now`: the timer's tick, or the assistant waking after a sleep.
    Tick {
        /// Now.
        now: UnixSeconds,
        /// The local clock.
        zone: Zone,
    },
    /// An almanac event happened that one routine watches.
    Saw {
        /// The routine.
        routine: RoutineId,
        /// The event, as a handle.
        event: EventRef,
        /// Its kind.
        kind: String,
        /// Now.
        now: UnixSeconds,
    },
    /// The job for a run is on the board.
    Posted {
        /// The routine.
        routine: RoutineId,
        /// The job.
        job: JobId,
    },
    /// The helper could not take the job for a run.
    NotPosted(RoutineId),
    /// A job on the board ended.
    Ended {
        /// The job.
        job: JobId,
        /// How.
        end: JobEnd,
        /// Now.
        now: UnixSeconds,
    },
}

/// What the book asks its owner to carry out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum RoutineEffect {
    /// Write this record.
    Save(RoutineRecord),
    /// Post this job.
    Run(RunRequest),
    /// Tell the person.
    Say {
        /// The helper it is about.
        helper: HelperId,
        /// What to tell.
        notice: RoutineNotice,
    },
    /// A routine was made.
    Created(RoutineId),
}

/// A time rule that wants a systemd timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimerEntry {
    /// The routine.
    pub routine: RoutineId,
    /// Its rule.
    pub schedule: Schedule,
}

/// Every routine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoutineBook {
    routines: Vec<Routine>,
    /// The last number minted for a routine.
    next: u64,
}

fn fire(
    routine: &mut Routine,
    at: UnixSeconds,
    event: Option<EventRef>,
    fx: &mut Vec<RoutineEffect>,
) {
    routine.record(RunRecord {
        at,
        job: None,
        event: event.clone(),
        outcome: RunOutcome::Started,
    });
    fx.push(RoutineEffect::Run(RunRequest {
        routine: routine.id,
        helper: routine.helper.clone(),
        goal: routine.goal.clone(),
        event,
        budget: routine.budget,
    }));
}

fn save(routine: &Routine, fx: &mut Vec<RoutineEffect>) {
    fx.push(RoutineEffect::Save(RoutineRecord::Kept(Box::new(
        routine.clone(),
    ))));
}

impl RoutineBook {
    /// The book the records give back, the newest of each routine winning.
    pub fn restore(records: impl IntoIterator<Item = RoutineRecord>) -> Self {
        let mut book = RoutineBook::default();
        for record in records {
            match record {
                RoutineRecord::Kept(routine) => {
                    book.next = book.next.max(routine.id.0);
                    match book.routines.iter().position(|r| r.id == routine.id) {
                        Some(at) => book.routines[at] = *routine,
                        None => book.routines.push(*routine),
                    }
                }
                RoutineRecord::Removed { id, .. } => book.routines.retain(|r| r.id != id),
            }
        }
        // A run whose job was never posted (the process ended between the two) did not run.
        for run in book.routines.iter_mut().flat_map(|r| r.runs.iter_mut()) {
            if run.job.is_none() && run.outcome == RunOutcome::Started {
                run.outcome = RunOutcome::Skipped;
            }
        }
        book
    }

    /// One record per routine, as the book now stands.
    pub fn records(&self) -> Vec<RoutineRecord> {
        self.routines
            .iter()
            .map(|r| RoutineRecord::Kept(Box::new(r.clone())))
            .collect()
    }

    /// Every routine, oldest first.
    pub fn routines(&self) -> &[Routine] {
        &self.routines
    }

    /// One routine.
    pub fn routine(&self, id: RoutineId) -> Option<&Routine> {
        self.routines.iter().find(|r| r.id == id)
    }

    /// The routines that run at a time and are not paused: each wants a timer.
    pub fn timers(&self) -> Vec<TimerEntry> {
        self.routines
            .iter()
            .filter(|r| r.state == RoutineState::Active)
            .filter_map(|r| match &r.trigger {
                Trigger::At(schedule) => Some(TimerEntry {
                    routine: r.id,
                    schedule: *schedule,
                }),
                _ => None,
            })
            .collect()
    }

    /// The routines that wait for an event and are not paused, with what they wait for.
    pub fn watches(&self) -> Vec<(&Routine, &EventMatch)> {
        self.routines
            .iter()
            .filter(|r| r.state == RoutineState::Active)
            .filter_map(|r| match &r.trigger {
                Trigger::On(event) => Some((r, event)),
                _ => None,
            })
            .collect()
    }

    /// The jobs of runs that have not ended.
    pub fn running_jobs(&self) -> Vec<JobId> {
        self.routines
            .iter()
            .flat_map(|r| r.runs.iter())
            .filter(|run| run.outcome == RunOutcome::Started)
            .filter_map(|run| run.job)
            .collect()
    }

    /// The jobs that routines wait for, which have not started them yet.
    pub fn awaited_jobs(&self) -> Vec<JobId> {
        self.routines
            .iter()
            .filter(|r| r.state == RoutineState::Active)
            .filter_map(|r| match &r.trigger {
                Trigger::AfterJob(JobRef(job)) if r.after != Some(*job) => Some(*job),
                _ => None,
            })
            .collect()
    }

    /// How many runs have not ended.
    pub fn running(&self) -> usize {
        self.routines.iter().map(Routine::running).sum()
    }

    fn find(&mut self, id: RoutineId) -> Option<&mut Routine> {
        self.routines.iter_mut().find(|r| r.id == id)
    }

    fn add(&mut self, new: NewRoutine, now: UnixSeconds, zone: Zone, fx: &mut Vec<RoutineEffect>) {
        self.next += 1;
        let id = RoutineId(self.next);
        let next_due = match &new.trigger {
            Trigger::At(schedule) => Some(schedule.next_after(now, zone)),
            Trigger::On(_) | Trigger::AfterJob(_) => None,
        };
        let routine = Routine {
            id,
            helper: new.helper,
            goal: new.goal,
            trigger: new.trigger,
            budget: new.budget,
            missed: new.missed,
            state: RoutineState::Active,
            next_due,
            after: None,
            failed: 0,
            runs: Vec::new(),
        };
        save(&routine, fx);
        fx.push(RoutineEffect::Created(id));
        self.routines.push(routine);
    }

    fn pause(&mut self, id: RoutineId, fx: &mut Vec<RoutineEffect>) {
        if let Some(routine) = self.find(id)
            && routine.state == RoutineState::Active
        {
            routine.state = RoutineState::Paused(PauseWhy::ByPerson);
            save(routine, fx);
        }
    }

    fn resume(&mut self, id: RoutineId, now: UnixSeconds, zone: Zone, fx: &mut Vec<RoutineEffect>) {
        if let Some(routine) = self.find(id)
            && matches!(routine.state, RoutineState::Paused(_))
        {
            routine.state = RoutineState::Active;
            routine.failed = 0;
            // The times that passed while it was paused are not run.
            if let Trigger::At(schedule) = &routine.trigger {
                routine.next_due = Some(schedule.next_after(now, zone));
            }
            save(routine, fx);
        }
    }

    fn remove(&mut self, id: RoutineId, fx: &mut Vec<RoutineEffect>) {
        if let Some(at) = self.routines.iter().position(|r| r.id == id) {
            let gone = self.routines.remove(at);
            fx.push(RoutineEffect::Save(RoutineRecord::Removed {
                id,
                helper: gone.helper,
            }));
        }
    }

    fn tick(&mut self, now: UnixSeconds, zone: Zone, fx: &mut Vec<RoutineEffect>) {
        for routine in &mut self.routines {
            let Trigger::At(schedule) = &routine.trigger else {
                continue;
            };
            let schedule = *schedule;
            if routine.state != RoutineState::Active {
                continue;
            }
            let Some(due) = routine.next_due else {
                routine.next_due = Some(schedule.next_after(now, zone));
                save(routine, fx);
                continue;
            };
            if due.0 > now.0 {
                continue;
            }
            routine.next_due = Some(schedule.next_after(now, zone));
            match (now.0 - due.0 > LATE, routine.missed) {
                // The time came while nothing was running: it is skipped, and recorded as such.
                (true, Missed::Skip) => routine.record(RunRecord {
                    at: due,
                    job: None,
                    event: None,
                    outcome: RunOutcome::Skipped,
                }),
                (false, _) | (true, Missed::RunOnceOnWake) => fire(routine, now, None, fx),
            }
            save(routine, fx);
        }
    }

    fn saw(
        &mut self,
        id: RoutineId,
        event: EventRef,
        kind: &str,
        now: UnixSeconds,
        fx: &mut Vec<RoutineEffect>,
    ) {
        let Some(routine) = self.find(id) else {
            return;
        };
        let hit = routine.state == RoutineState::Active
            && matches!(&routine.trigger, Trigger::On(m) if m.covers(kind, &event.space));
        if hit {
            fire(routine, now, Some(event), fx);
            save(routine, fx);
        }
    }

    fn posted(&mut self, id: RoutineId, job: JobId, fx: &mut Vec<RoutineEffect>) {
        let Some(routine) = self.find(id) else {
            return;
        };
        if let Some(run) = routine
            .runs
            .iter_mut()
            .rev()
            .find(|run| run.job.is_none() && run.outcome == RunOutcome::Started)
        {
            run.job = Some(job);
        }
        save(routine, fx);
    }

    fn not_posted(&mut self, id: RoutineId, fx: &mut Vec<RoutineEffect>) {
        let Some(routine) = self.find(id) else {
            return;
        };
        if let Some(run) = routine
            .runs
            .iter_mut()
            .rev()
            .find(|run| run.job.is_none() && run.outcome == RunOutcome::Started)
        {
            run.outcome = RunOutcome::Skipped;
        }
        save(routine, fx);
    }

    fn ended(&mut self, job: JobId, end: JobEnd, now: UnixSeconds, fx: &mut Vec<RoutineEffect>) {
        for routine in &mut self.routines {
            let Some(run) = routine
                .runs
                .iter_mut()
                .find(|run| run.job == Some(job) && run.outcome == RunOutcome::Started)
            else {
                continue;
            };
            match end {
                JobEnd::Done => {
                    run.outcome = RunOutcome::Done;
                    routine.failed = 0;
                }
                JobEnd::Failed(fault) => {
                    run.outcome = RunOutcome::Failed(fault);
                    routine.failed = routine.failed.saturating_add(1);
                    if routine.failed >= IN_A_ROW && routine.state == RoutineState::Active {
                        routine.state = RoutineState::Paused(PauseWhy::Failed3);
                        fx.push(RoutineEffect::Say {
                            helper: routine.helper.clone(),
                            notice: RoutineNotice::Paused {
                                when: routine.trigger.plain(),
                            },
                        });
                    }
                }
            }
            save(routine, fx);
        }
        if matches!(end, JobEnd::Done) {
            for routine in &mut self.routines {
                if routine.state == RoutineState::Active
                    && routine.trigger == Trigger::AfterJob(JobRef(job))
                    && routine.after != Some(job)
                {
                    routine.after = Some(job);
                    fire(routine, now, None, fx);
                    save(routine, fx);
                }
            }
        }
    }
}

/// Moves the book by `event` and returns it with what to carry out.
pub fn step(mut book: RoutineBook, event: RoutineEvent) -> (RoutineBook, Vec<RoutineEffect>) {
    let mut fx = Vec::new();
    match event {
        RoutineEvent::Add { new, now, zone } => book.add(*new, now, zone, &mut fx),
        RoutineEvent::Pause(id) => book.pause(id, &mut fx),
        RoutineEvent::Resume { id, now, zone } => book.resume(id, now, zone, &mut fx),
        RoutineEvent::Remove(id) => book.remove(id, &mut fx),
        RoutineEvent::Tick { now, zone } => book.tick(now, zone, &mut fx),
        RoutineEvent::Saw {
            routine,
            event,
            kind,
            now,
        } => book.saw(routine, event, &kind, now, &mut fx),
        RoutineEvent::Posted { routine, job } => book.posted(routine, job, &mut fx),
        RoutineEvent::NotPosted(id) => book.not_posted(id, &mut fx),
        RoutineEvent::Ended { job, end, now } => book.ended(job, end, now, &mut fx),
    }
    (book, fx)
}
