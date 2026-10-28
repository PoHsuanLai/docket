//! The helper desk's decisions, as functions of the board and the words asked: whether a name is
//! free, the card a form makes, the job a person or a planner asks for, the routine a proposer
//! suggests, what the board's effects came to, and what a helper's turn answered. The hub carries
//! the results out; the refusals are typed ([`Refusal`]).

use crate::answers::Answers;
use crate::job::{JobEnding, JobReport, JobSpec};
use crate::refusal::Refusal;
use companion_model::helper::{
    Board, BoardEffect, BoardEvent, CardDraft, Ceiling, GoalOrigin, GoalText, HelperBudget,
    HelperCard, HelperKind, HelperModel, HelperName, HelperState, JobFault, JobId, JobRefusal,
    JobStatus, Launch, NewJob, ReachFault, Role, reach_policy,
};
use companion_model::routine::{JobEnd, Missed, Schedule};
use companion_wire::{HelperForm, RoutineForm, WhenForm, line_of};
use docket_core::{HelperRosterLine, LeadText};
use docket_manifest::Manifest;
use porter_core::AppName;
use porter_ids::Count;
use prov::{EntityId, HelperId, SpaceId, TaskId};

/// How many steps a helper the assistant proposed may take in one job, until the person says.
pub const PROPOSED_STEPS: Count = Count(12);

/// The longest sentence of why a proposer may give.
const LONGEST_WHY: usize = 200;

/// A name no other card has: `except` is the card being edited, which may keep its own.
pub fn name_free(
    board: &Board,
    name: &HelperName,
    except: Option<&HelperId>,
) -> Result<(), Refusal> {
    match board
        .by_name(name.as_str())
        .filter(|card| Some(&card.id) != except)
    {
        Some(_) => Err(Refusal::NameTaken(name.clone())),
        None => Ok(()),
    }
}

/// The card the person confirmed, from the form: the reach is built from the apps named, over
/// the apps installed.
pub fn card_draft(
    form: HelperForm,
    space: &SpaceId,
    installed: &[&Manifest],
) -> Result<CardDraft, Refusal> {
    let reach = reach_policy(&form.apps, form.ceiling, space, installed).map_err(Refusal::Reach)?;
    Ok(CardDraft {
        name: form.name,
        role: form.role,
        kind: form.kind,
        space: space.clone(),
        model: form.model,
        reach,
        budget: HelperBudget {
            steps: Count(form.steps.0.max(1)),
        },
    })
}

/// The form for a helper the assistant proposes: its own planner, no model until the person
/// chooses, the apps named by their names.
pub fn helper_form(
    name: &str,
    role: &str,
    apps: &[String],
    ceiling: Ceiling,
    space: SpaceId,
) -> Result<HelperForm, Refusal> {
    let (Ok(name), Ok(role)) = (HelperName::parse(name), Role::parse(role)) else {
        return Err(Refusal::BadNameOrRole);
    };
    let apps = apps
        .iter()
        .map(|app| {
            AppName::parse(app).map_err(|_| Refusal::Reach(ReachFault::NoSuchApp(app.clone())))
        })
        .collect::<Result<Vec<AppName>, Refusal>>()?;
    Ok(HelperForm {
        name,
        role,
        space,
        model: HelperModel::Unset,
        apps,
        ceiling,
        steps: PROPOSED_STEPS,
        kind: HelperKind::Native,
    })
}

/// The first card made among `effects`.
pub fn created(effects: &[BoardEffect]) -> Result<HelperId, Refusal> {
    effects
        .iter()
        .find_map(|effect| match effect {
            BoardEffect::Created(id) => Some(id.clone()),
            _ => None,
        })
        .ok_or(Refusal::HelperNotMade)
}

/// What a job asked for came to: the job made, or why not.
pub fn posted(effects: &[BoardEffect]) -> Result<JobId, Refusal> {
    for effect in effects {
        match effect {
            BoardEffect::JobRefused(why) => return Err(Refusal::Job(why.clone())),
            BoardEffect::Posted(id) => return Ok(*id),
            _ => {}
        }
    }
    Err(Refusal::JobNotGiven)
}

/// The job the person gives `helper`, in their words, for the things it alone may write.
pub fn person_job(
    board: &Board,
    helper: &HelperId,
    goal: &str,
    things: Vec<EntityId>,
) -> Result<NewJob, Refusal> {
    if board.card(helper).is_none() {
        return Err(Refusal::Job(JobRefusal::NoSuchHelper));
    }
    let goal = GoalText::parse(goal).map_err(|_| Refusal::NoGoal)?;
    Ok(NewJob {
        to: helper.clone(),
        goal,
        origin: GoalOrigin::Person,
        giver: None,
        owns: things,
        with: Vec::new(),
    })
}

/// A job a planner gives a helper by its name.
#[derive(Debug, Clone)]
pub struct PlannerJob<'a> {
    /// The name of the helper it is for.
    pub helper: &'a str,
    /// The Space the giver works in: a helper in another Space is not reachable from it.
    pub space: &'a SpaceId,
    /// What it is to do.
    pub goal: &'a str,
    /// The name of a helper whose latest finished job's answer it is handed, as data.
    pub after: Option<&'a str>,
    /// The task of the planner giving it.
    pub giver: &'a TaskId,
    /// Where the router found the goal to come from.
    pub origin: GoalOrigin,
}

/// The card of the helper called `name` in `space`: Spaces are walls.
fn named_in<'a>(board: &'a Board, name: &str, space: &SpaceId) -> Option<&'a HelperCard> {
    board.by_name(name).filter(|card| &card.space == space)
}

/// The latest job `name` finished.
fn latest_answer(board: &Board, name: &str) -> Option<JobId> {
    let card = board.by_name(name)?;
    board
        .jobs()
        .iter()
        .rev()
        .find(|j| j.owner == card.id && matches!(j.status, JobStatus::Done(_)))
        .map(|j| j.id)
}

/// The job a planner gives a helper. The router refused this unless the planner had read nothing
/// untrusted; the board asks again.
pub fn planner_job(board: &Board, gift: PlannerJob<'_>) -> Result<NewJob, Refusal> {
    let card =
        named_in(board, gift.helper, gift.space).ok_or(Refusal::Job(JobRefusal::NoSuchHelper))?;
    let goal = GoalText::parse(gift.goal).map_err(|_| Refusal::NoGoal)?;
    let with = match gift.after {
        None => Vec::new(),
        Some(name) => vec![
            latest_answer(board, name).ok_or_else(|| Refusal::NothingToHandOn(name.to_owned()))?,
        ],
    };
    Ok(NewJob {
        to: card.id.clone(),
        goal,
        origin: gift.origin,
        giver: Some(gift.giver.clone()),
        owns: Vec::new(),
        with,
    })
}

/// Which helper a proposed routine is for.
#[derive(Debug, Clone)]
pub enum RoutineWho<'a> {
    /// The one named, by the name the person gave it, in the proposer's Space.
    Named(&'a str, &'a SpaceId),
    /// The helper whose planner is proposing: a helper proposes for its own card only.
    Itself(&'a HelperId),
}

/// A routine a proposer suggests.
#[derive(Debug, Clone)]
pub struct RoutineWish<'a> {
    /// Who it is for.
    pub who: RoutineWho<'a>,
    /// What it is to do.
    pub goal: &'a str,
    /// When, in plain words ("daily 08:00", "every 2 hours").
    pub when: &'a str,
    /// The proposer's one sentence on why.
    pub why: &'a str,
}

/// The routine a proposer suggests, as the form the person would confirm, with its why. Nothing
/// is made: the goal, the time and the why are a model's words, shown to the person as data.
pub fn routine_wish(
    board: &Board,
    wish: RoutineWish<'_>,
) -> Result<(RoutineForm, String), Refusal> {
    let card = match wish.who {
        RoutineWho::Named(name, space) => named_in(board, name, space),
        RoutineWho::Itself(id) => board.card(id),
    }
    .ok_or(Refusal::Job(JobRefusal::NoSuchHelper))?;
    let goal = GoalText::parse(wish.goal).map_err(|_| Refusal::NoGoal)?;
    let when = wish.when.trim();
    Schedule::parse(when).map_err(Refusal::Schedule)?;
    let why = wish.why.trim();
    if why.is_empty() || why.chars().count() > LONGEST_WHY {
        return Err(Refusal::NeedWhy);
    }
    let form = RoutineForm {
        helper: card.id.clone(),
        goal: goal.as_str().to_owned(),
        when: WhenForm::At(when.to_owned()),
        missed: Missed::Skip,
        steps: PROPOSED_STEPS.0,
    };
    Ok((form, why.to_owned()))
}

/// Whether the person may talk with `card` in a conversation of docket's: a helper that runs its
/// own app is talked with there.
pub fn talkable(card: &HelperCard) -> Result<(), Refusal> {
    match card.kind.program() {
        Some(_) => Err(Refusal::RunsItsOwnApp(card.name.clone())),
        None => Ok(()),
    }
}

/// What a helper's turn in its conversation answered: its words, or why it did not.
pub fn say_outcome(card: &HelperCard, report: JobReport) -> Result<String, Refusal> {
    let JobReport { ending, why, .. } = report;
    match ending {
        JobEnding::Done { words } => Ok(words),
        JobEnding::Failed(fault) => Err(Refusal::SayFailed {
            name: card.name.clone(),
            fault,
            why,
        }),
        JobEnding::Stopped => Err(Refusal::SayStopped(card.name.clone())),
        JobEnding::NeedsSignIn(_) => Err(Refusal::SayNeedsSignIn(card.name.clone())),
    }
}

/// The event that resumes a paused helper, or asks another to try again.
pub fn resume_event(board: &Board, helper: HelperId) -> BoardEvent {
    let paused = matches!(
        board.card(&helper).map(|c| &c.state),
        Some(HelperState::Paused(_))
    );
    match paused {
        true => BoardEvent::Resume(helper),
        false => BoardEvent::Retry(helper),
    }
}

/// The helpers a planner is told of, except `except` (the helper whose job it is, if it is one).
pub fn roster_lines(board: &Board, except: Option<&HelperId>) -> Vec<HelperRosterLine> {
    board
        .cards()
        .iter()
        .filter(|card| Some(&card.id) != except)
        .map(|card| HelperRosterLine {
            name: card.name.clone(),
            space: card.space.clone(),
            state: card.state.phase(),
            doing: Some(LeadText::of(&line_of(card, board, None).doing)),
        })
        .collect()
}

/// The job as a run needs it: its card, the job, and the answers it is handed that are still
/// held.
pub fn job_spec(board: &Board, answers: &Answers, launch: &Launch) -> Option<JobSpec> {
    let card = board.card(&launch.helper)?.clone();
    let job = board.job(launch.job)?.clone();
    let inputs = answers.inputs_for(&job);
    Some(JobSpec { card, job, inputs })
}

/// How a job ended, once it has: done, or open again after a failure. A job the board no longer
/// holds did not finish.
pub fn job_end(board: &Board, job: JobId) -> Option<JobEnd> {
    let Some(found) = board.job(job) else {
        return Some(JobEnd::Failed(JobFault::Crashed));
    };
    match (&found.status, found.failed) {
        (JobStatus::Done(_), _) => Some(JobEnd::Done),
        (_, Some(fault)) => Some(JobEnd::Failed(fault)),
        _ => None,
    }
}
