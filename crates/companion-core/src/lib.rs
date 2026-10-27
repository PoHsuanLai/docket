//! The companion's pure logic: what companiond decides, with no runtime, bus, file or clock. A
//! runner (companiond today, `docket-helpers` later) owns the state and the tasks, carries the
//! decisions out, and gives this crate the time as a value.
//!
//! - [`Speaker`], [`Call`], [`permits`]: who may make which call.
//! - [`conclude`], [`Conclusion`], [`Finished`], [`Answer`], [`Hold`]: how a job's end is told to
//!   the board; [`JobSpec`], [`JobReport`], [`JobEnding`], [`OpenedJob`], [`JobInput`]: a job as a
//!   value.
//! - [`Answers`], [`ToldLog`], [`Told`], [`Proposals`], [`Logins`], [`Terminals`]: what the
//!   helper board keeps beside the board, as plain state.
//! - [`Refusal`]: why a desk said no, as a type; the words are the edge's.
//! - [`name_free`], [`card_draft`], [`person_job`], [`planner_job`], [`routine_wish`] and the
//!   rest of the desk's decisions.
//! - [`by_space`], [`group_by_space`], [`space_of`]: records and watches grouped by Space.
//! - [`units_of`], [`UnitFile`], [`offset_of`], [`ZoneReading`], [`EventWatch`], [`event_asks`],
//!   [`Settle`], [`ended_events`], [`new_routine`]: the routines' scheduler, as values.

mod answers;
mod conclude;
mod desk;
mod job;
mod login;
mod proposals;
mod records;
mod refusal;
mod routines;
mod speaker;
mod terminal;
mod told;

pub use answers::{Answers, KEPT_ANSWERS};
pub use conclude::{Answer, Conclusion, Finished, Hold, conclude};
pub use desk::{
    PROPOSED_STEPS, PlannerJob, RoutineWho, RoutineWish, card_draft, created, helper_form, job_end,
    job_spec, name_free, person_job, planner_job, posted, resume_event, roster_lines, routine_wish,
    say_outcome, talkable,
};
pub use job::{JobEnding, JobInput, JobReport, JobSpec, OpenedJob};
pub use login::{Logins, SameApp, same_app};
pub use proposals::{MOST_WAITING, Proposals};
pub use records::{by_space, group_by_space, space_of};
pub use refusal::Refusal;
pub use routines::desk::new_routine;
pub use routines::due::ended_events;
pub use routines::settle::{Awake, Settle, Settled};
pub use routines::units::{UNIT_PREFIX, UnitFile, units_of};
pub use routines::watch::{EventAsk, EventWatch, Seen, SpaceWatch, event_asks};
pub use routines::zone::{ZoneReading, offset_of};
pub use speaker::{Call, Speaker, permits};
pub use terminal::{Terminals, terminal_sessions, why_not_to_terminal};
pub use told::{MOST_NOTICES, Told, ToldLog};
