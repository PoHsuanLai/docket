//! Helpers: persistent, named agents the person keeps, several working at once. A helper is a
//! standing grant of attention, so it exists only when the person confirms its card; each job it
//! does is an ordinary task under a policy that is never wider than the helper's `reach`.
//!
//! This module is the pure half: the card ([`HelperCard`]), the board of jobs and the mailbox of
//! posts ([`Board`], moved by [`step`]), the rules that keep two channels apart (what a helper is
//! told to do comes from the person's words or from a planner that read nothing untrusted;
//! everything else travels as a handle), and the records that make the board survive a restart.
//! Nothing here reaches a clock, a bus or a model: companiond owns the board and carries out its
//! effects.
//!
//! - [`HelperName`], [`Role`], [`HelperKind`], [`HelperModel`], [`HelperBudget`], [`HelperCard`],
//!   [`HelperState`], [`PauseWhy`], [`Need`], [`TaskRef`]: the card.
//! - [`Job`], [`JobId`], [`GoalText`], [`GoalSource`], [`GoalOrigin`], [`JobStatus`],
//!   [`ResultHandle`], [`JobFault`]: the board's jobs.
//! - [`Post`], [`PostId`], [`PostTo`], [`PostPart`]: the mailbox.
//! - [`Board`], [`BoardEvent`], [`BoardEffect`], [`step`]: the state machine.
//! - [`HelperRecord`], [`HelperRecordKind`]: what is stored and restored.
//! - [`HelperSeat`]: what the router is told when a job's session opens.

mod board;
mod card;
mod ids;
mod job;
mod notice;
mod post;
mod records;
mod rules;
mod seat;

pub use board::{Board, BoardEffect, BoardEvent, CallVerdict, Launch, NewJob, step};
pub use card::{
    CardDraft, HelperBudget, HelperCard, HelperKind, HelperModel, HelperName, HelperState,
    HelperTextFault, Need, PauseWhy, Role, TaskRef,
};
pub use job::{
    GoalFault, GoalOrigin, GoalSource, GoalText, Job, JobFault, JobId, JobStatus, PlannerSaw,
    ResultHandle,
};
pub use notice::{Notice, owned_sentence};
pub use post::{Post, PostId, PostPart, PostTo};
pub use records::{CardRecord, HelperRecord, HelperRecordKind};
pub use rules::{JobRefusal, admit_goal};
pub use seat::HelperSeat;
