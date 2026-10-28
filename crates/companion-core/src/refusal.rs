//! Why the desks of the helpers and the routines said no, as a type. The sentences the person is
//! told are made once, at the edge that serves them (companiond today, the view crate later); a
//! decision here never carries words.

use companion_model::helper::{HandFault, HelperName, JobFault, JobRefusal, ReachFault};
use companion_model::routine::ScheduleFault;

/// What a request to the helper's app was for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppStep {
    /// Open the app's terminal.
    Terminal,
    /// Sign in to the app.
    SignIn,
    /// Sign out of the app.
    SignOut,
}

/// Why a request to a desk was not carried out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The board would not make the job.
    Job(JobRefusal),
    /// A conversation could not move to or from a terminal.
    Hand(HandFault),
    /// What the person named is not a reach.
    Reach(ReachFault),
    /// The words are not a schedule.
    Schedule(ScheduleFault),
    /// Another helper already has this name.
    NameTaken(HelperName),
    /// The goal is empty or too long to be one.
    NoGoal,
    /// The helper named has no finished job to hand on.
    NothingToHandOn(String),
    /// The board made no job, and said nothing about it.
    JobNotGiven,
    /// A proposal came without a short why.
    NeedWhy,
    /// Too many proposed routines wait for an answer.
    TooManyWaiting,
    /// The proposed name, or what it is for, is empty or too long.
    BadNameOrRole,
    /// The board made no helper.
    HelperNotMade,
    /// The session is not a conversation with a helper.
    NotAConversation,
    /// The helper runs its own app, so the person talks with it there.
    RunsItsOwnApp(HelperName),
    /// A helper's turn did not finish; the model may have said why.
    SayFailed {
        /// The helper.
        name: HelperName,
        /// How the turn failed.
        fault: JobFault,
        /// A sentence on why, when the model's refusal gave one.
        why: Option<String>,
    },
    /// The helper's turn was stopped.
    SayStopped(HelperName),
    /// The helper's app wants the person to sign in first.
    SayNeedsSignIn(HelperName),
    /// The helper does not run an app, so there is nothing to do with it at this step.
    NotAnApp(AppStep),
    /// No host of apps can be reached for this step (never a sign-out: that needs only the app).
    NoAppHost(AppStep),
    /// The conversation is already in the terminal.
    AlreadyInTerminal,
    /// The way chosen is not one the app listed.
    NotAWayToSignIn,
    /// The app did not sign the person in.
    SignInDeclined,
    /// The app could not be started to sign in.
    SignInNotStarted,
    /// Signing out did not work.
    SignOutFailed,
    /// A routine was asked for a helper that is not there.
    UnknownHelper,
    /// The kind of thing to watch for is not one.
    WatchKind,
    /// Nothing was said to watch for.
    WatchWords,
    /// The routine book made no routine.
    RoutineNotMade,
}
