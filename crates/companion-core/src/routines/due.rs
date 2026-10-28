//! What a tick of the scheduler finds: the runs that ended, and the jobs routines wait for that
//! ended done.

use companion_model::helper::JobId;
use companion_model::routine::{JobEnd, RoutineBook, RoutineEvent};
use prov::UnixSeconds;

/// The events for the jobs that ended by `now`: each run of the book that has not ended and whose
/// job `end_of` says has, then each job a routine waits for that ended done (a job a routine waits
/// for starts it only by ending done).
pub fn ended_events(
    book: &RoutineBook,
    now: UnixSeconds,
    end_of: impl Fn(JobId) -> Option<JobEnd>,
) -> Vec<RoutineEvent> {
    let running = book
        .running_jobs()
        .into_iter()
        .filter_map(|job| Some((job, end_of(job)?)));
    let awaited = book
        .awaited_jobs()
        .into_iter()
        .filter(|job| end_of(*job) == Some(JobEnd::Done))
        .map(|job| (job, JobEnd::Done));
    running
        .chain(awaited)
        .map(|(job, end)| RoutineEvent::Ended { job, end, now })
        .collect()
}
