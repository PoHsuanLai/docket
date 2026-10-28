//! Routines: a helper's standing instructions, run without being asked. A routine is a goal in the
//! person's words (Trusted, written when the routine is made) and what starts it: a time, an
//! almanac event, or another job ending done. A run is an ordinary job on the helper's board; the
//! last 20 runs are kept, and three that do not finish in a row pause the routine.
//!
//! This is the pure half. Nothing here reads a clock, a bus or a timer: companiond owns the book,
//! passes the time and the local zone in, and carries out the effects.
//!
//! - [`Schedule`], [`TimeOfDay`], [`Hours`], [`Zone`], [`ScheduleFault`]: the time rules.
//! - [`Trigger`], [`EventMatch`], [`JobRef`]: what starts a routine.
//! - [`Routine`], [`RoutineId`], [`RoutineState`], [`RoutinePause`], [`Missed`], [`RunBudget`],
//!   [`RunRecord`], [`RunOutcome`]: the routine and its runs.
//! - [`RoutineBook`], [`RoutineEvent`], [`RoutineEffect`], [`step`]: the state machine.
//! - [`RoutineRecord`]: what is stored and restored.

mod book;
mod kept;
mod schedule;
mod shapes;
mod trigger;

pub use book::{LATE, RoutineBook, RoutineEffect, RoutineEvent, TimerEntry, step};
pub use kept::{
    MOST_RUNS, Missed, Routine, RoutineId, RoutinePause, RoutineState, RunBudget, RunOutcome,
    RunRecord,
};
pub use schedule::{Hours, Schedule, ScheduleFault, TimeOfDay, Zone};
pub use shapes::{JobEnd, NewRoutine, RoutineNotice, RoutineRecord, RunRequest};
pub use trigger::{EventMatch, JobRef, Trigger};
