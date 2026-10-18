//! The desk: how the task model reaches the helpers companiond owns. The board is not here (it is
//! `docket_core::helper`'s, and companiond owns it); this is the seam a host passes in so that
//! the planner's two helper actions and the shell's helper calls reach it. A host with no helpers
//! passes none, and the actions are refused.

use companion_wire::{
    HelperCardView, HelperForm, HelperReply, HelpersView, ProposalId, RoutineProposal,
};
use docket_core::HelperRosterLine;
use docket_core::helper::Ceiling;
use prov::{EntityId, HelperId, SpaceId, TaskId};
use std::fmt;
use std::future::Future;
use std::pin::Pin;

/// A call that has not finished.
pub type Pending<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A job the assistant (or another helper's planner) gives to a helper by its name. The router
/// has already refused it unless the goal came from a planner that had read nothing untrusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostFromPlanner {
    /// The name of the helper it is for.
    pub helper: String,
    /// What it is to do.
    pub goal: String,
    /// The name of a helper whose latest finished job's answer it is handed, as data.
    pub after: Option<String>,
    /// The task of the planner giving it: the job's policy is cut from that task's.
    pub giver: TaskId,
    /// The Space the giver works in: a helper in another Space is not reachable from it.
    pub space: SpaceId,
}

/// A job the person gives to a helper, through the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostFromPerson {
    /// The helper.
    pub helper: HelperId,
    /// What it is to do, in their words.
    pub goal: String,
    /// The things it alone may write.
    pub things: Vec<EntityId>,
}

/// A helper the assistant proposes. Nothing exists until the person confirms the sheet the router
/// asked; by the time this reaches the desk, they have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// What to call it.
    pub name: String,
    /// What it is for.
    pub role: String,
    /// The apps it may use, by name.
    pub apps: Vec<String>,
    /// How far it may go in them.
    pub ceiling: Ceiling,
    /// The Space of the conversation that proposed it.
    pub space: SpaceId,
}

/// Which helper a proposed routine is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutineFor {
    /// The one the assistant named, by the name the person gave it.
    Named(String),
    /// The helper whose planner is proposing: a helper proposes for its own card only.
    Itself(HelperId),
}

/// A routine the assistant or a helper proposes. Nothing is made: it waits on a sheet for the
/// person, and `why` is the proposer's sentence, a model's words, kept as data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutineAsk {
    /// Who it is for.
    pub helper: RoutineFor,
    /// What it is to do.
    pub goal: String,
    /// When, in plain words ("daily 08:00", "every 2 hours").
    pub when: String,
    /// The proposer's one sentence on why.
    pub why: String,
    /// The Space of the conversation that proposed it: a helper in another Space is not reachable.
    pub space: SpaceId,
}

/// What the task model asks of the helpers a host keeps.
pub trait HelperDesk: fmt::Debug + Send + Sync {
    /// The list, with what was told after the sentence numbered `after`.
    fn view(&self, after: u64) -> HelpersView;

    /// One helper's card.
    fn card(&self, helper: &HelperId) -> Option<HelperCardView>;

    /// Makes a helper from what the person filled in.
    fn create(&self, form: HelperForm) -> Pending<'_, HelperReply>;

    /// Changes a helper to what the person filled in.
    fn edit(&self, helper: HelperId, form: HelperForm) -> Pending<'_, HelperReply>;

    /// The person pauses a helper.
    fn pause(&self, helper: HelperId) -> Pending<'_, ()>;

    /// The person resumes a helper, or asks it to try again.
    fn resume(&self, helper: HelperId) -> Pending<'_, ()>;

    /// The person removes a helper.
    fn remove(&self, helper: HelperId) -> Pending<'_, ()>;

    /// The person gives a helper a job.
    fn post_job(&self, ask: PostFromPerson) -> Pending<'_, HelperReply>;

    /// A planner gives a helper a job.
    fn post_from_planner(&self, ask: PostFromPlanner) -> Pending<'_, HelperReply>;

    /// The assistant proposes a helper, and the person has confirmed it.
    fn propose(&self, proposal: Proposal) -> Pending<'_, HelperReply>;

    /// The assistant or a helper proposes a routine; it waits for the person. Kept in memory only.
    fn propose_routine(&self, ask: RoutineAsk) -> Pending<'_, HelperReply>;

    /// The routines proposed that wait for the person.
    fn routine_proposals(&self) -> Vec<RoutineProposal>;

    /// Takes a proposal off the list, to accept or to decline it.
    fn take_routine_proposal(&self, id: ProposalId) -> Option<RoutineProposal>;

    /// The person chooses one of the ways of signing in to the app the helper runs (`way` is the
    /// id of a `SignInChoice` on its card). Answers `Done` once the app is signed in, or
    /// `OpenTerminal` when the way runs in a terminal window the shell opens.
    fn sign_in(&self, helper: HelperId, way: String) -> Pending<'_, HelperReply>;

    /// The person signs out of the app the helper runs. Every helper that runs the same app is
    /// signed out with it: the app has one login.
    fn sign_out(&self, helper: HelperId) -> Pending<'_, HelperReply>;

    /// The helpers a planner is told of, except `except` (the helper whose job it is, if it is
    /// one).
    fn roster_lines(&self, except: Option<&HelperId>) -> Vec<HelperRosterLine>;
}
