//! The board: every helper's card, every job and every post, moved by one pure function.
//! companiond owns the board, so taking a job is decided here and needs no compare-and-set; it
//! carries out the effects `step` returns (writing the records, starting the task, telling the
//! person) and feeds back what happened.

mod moves;

use crate::helper::card::{CardDraft, HelperCard, HelperName, HelperState, PauseWhy, TaskRef};
use crate::helper::ids::helper_number;
use crate::helper::job::{GoalOrigin, GoalText, Job, JobFault, JobId, JobStatus, ResultHandle};
use crate::helper::notice::Notice;
use crate::helper::post::{Post, PostId, PostPart, PostTo};
use crate::helper::records::{CardRecord, HelperRecord};
use crate::helper::rules::JobRefusal;
use almanac_core::EventRef;
use porter_ids::Count;
use prov::{EntityId, HelperId, Label, TaskId};
use serde::{Deserialize, Serialize};

/// How many refusals in a row, or failed jobs in a row, pause a helper.
const IN_A_ROW: u8 = 3;

/// Consecutive refusals of a helper's calls, and consecutive jobs of it that failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Streak {
    helper: HelperId,
    denied: u8,
    failed: u8,
}

/// Everything the board holds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Board {
    cards: Vec<HelperCard>,
    jobs: Vec<Job>,
    posts: Vec<Post>,
    streaks: Vec<Streak>,
    /// The last number minted for a helper, job or post.
    next: u64,
}

/// A job someone wants made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewJob {
    /// The helper it is for.
    pub to: HelperId,
    /// What it is to do.
    pub goal: GoalText,
    /// Who is giving it, as far as the goal's trust goes.
    pub origin: GoalOrigin,
    /// The task of the planner giving it; the job's policy is cut from that task's.
    pub giver: Option<TaskId>,
    /// The things it writes.
    pub owns: Vec<EntityId>,
    /// Finished jobs whose answers it is handed as data.
    pub with: Vec<JobId>,
}

/// What a routine's run adds to the job it posts.
#[derive(Debug, Default)]
struct RunTerms {
    /// The event that started it.
    about: Option<EventRef>,
    /// Its step budget.
    steps: Option<Count>,
}

/// How a helper's call ended, as far as the breaker goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallVerdict {
    /// It ran, or asked the person and was answered.
    Allowed,
    /// A rule refused it.
    Denied,
}

/// Something happened to the board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum BoardEvent {
    /// The person confirmed a new card.
    Create(Box<CardDraft>),
    /// The person edited a card.
    Edit {
        /// Which.
        helper: HelperId,
        /// The card as confirmed.
        draft: Box<CardDraft>,
    },
    /// The person paused a helper.
    Pause(HelperId),
    /// The person resumed a helper.
    Resume(HelperId),
    /// The person removed a helper.
    Remove(HelperId),
    /// A job is posted.
    Post(Box<NewJob>),
    /// A job is posted for a routine's run: the event that started it, if one did, goes with the
    /// job as a handle (no part of it is in the goal), and the run's step budget bounds the job.
    PostRun {
        /// The job.
        job: Box<NewJob>,
        /// The event.
        about: Option<EventRef>,
        /// The most steps the run takes.
        steps: Count,
    },
    /// The helper is free: give it its oldest open job, unless that job failed before.
    Take(HelperId),
    /// The person asks the helper to try again: give it its oldest open job, even one that
    /// failed before.
    Retry(HelperId),
    /// A job's task is running.
    Started {
        /// The job.
        job: JobId,
        /// Its task.
        task: TaskId,
    },
    /// A job finished.
    Finished {
        /// The job.
        job: JobId,
        /// Where its answer is held.
        result: ResultHandle,
    },
    /// A job did not finish.
    Failed {
        /// The job.
        job: JobId,
        /// Why.
        fault: JobFault,
    },
    /// A job could not start because the agent app its helper runs is not signed in. Nothing
    /// failed: the job waits, and so does the helper, until the person signs in.
    NeedsSignIn(JobId),
    /// The agent app of this helper is signed in: its waiting job is open again.
    SignedIn(HelperId),
    /// A job was stopped before it finished, at the person's word: it is open again, and
    /// nothing went wrong.
    Stopped(JobId),
    /// One of a helper's calls ended.
    Called {
        /// The helper.
        helper: HelperId,
        /// How.
        verdict: CallVerdict,
    },
    /// A helper hands something to another, or to all.
    Hand {
        /// The sender.
        from: HelperId,
        /// Who it is for.
        to: PostTo,
        /// What it carries.
        parts: Vec<PostPart>,
        /// The join of what the sender had read.
        label: Label,
    },
    /// A post reached a helper.
    Delivered {
        /// The post.
        post: PostId,
        /// The helper.
        helper: HelperId,
    },
}

/// A job to start for a helper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launch {
    /// The helper.
    pub helper: HelperId,
    /// Its job, now taken.
    pub job: JobId,
}

/// What the host does because of an event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum BoardEffect {
    /// Write this record.
    Save(HelperRecord),
    /// A card was made; its id.
    Created(HelperId),
    /// A job was made; its id.
    Posted(JobId),
    /// A job was not made.
    JobRefused(JobRefusal),
    /// Start this job's task.
    Launch(Launch),
    /// Stop this running task: its helper was paused or removed.
    Stop(TaskRef),
    /// Tell the person, once.
    Say {
        /// The helper it is about.
        helper: HelperId,
        /// Its name now.
        name: HelperName,
        /// What to tell.
        notice: Notice,
    },
}

/// Whether a job that failed before is tried again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Retry {
    /// Only jobs that have not failed: a failure waits for the person or the next explicit take.
    Fresh,
    /// Any open job.
    Again,
}

fn save_card(card: &HelperCard) -> BoardEffect {
    BoardEffect::Save(HelperRecord::Card(CardRecord::Kept(Box::new(card.clone()))))
}

fn save_job(job: &Job) -> BoardEffect {
    BoardEffect::Save(HelperRecord::Job(Box::new(job.clone())))
}

impl Board {
    /// An empty board.
    pub fn new() -> Self {
        Self::default()
    }

    /// The board after a restart, from the records it wrote, oldest first (the newest record of a
    /// card, job or post wins). Nothing was running when the process ended: a helper that was
    /// working or waiting is idle; a job that was blocked is open; a job that was taken is open
    /// too but marked as stopped unexpectedly, so it waits for the person to say try again (it may
    /// have done part of its work); and a job whose helper is gone is dropped.
    pub fn restore(records: impl IntoIterator<Item = HelperRecord>) -> Self {
        let mut board = Board::default();
        for record in records {
            match record {
                HelperRecord::Card(CardRecord::Kept(card)) => {
                    if let Some(n) = helper_number(&card.id) {
                        board.next = board.next.max(n);
                    }
                    match board.cards.iter().position(|c| c.id == card.id) {
                        Some(at) => board.cards[at] = *card,
                        None => board.cards.push(*card),
                    }
                }
                HelperRecord::Card(CardRecord::Removed(id)) => {
                    board.cards.retain(|c| c.id != id);
                }
                HelperRecord::Job(job) => {
                    board.next = board.next.max(job.id.0);
                    match board.jobs.iter().position(|j| j.id == job.id) {
                        Some(at) => board.jobs[at] = *job,
                        None => board.jobs.push(*job),
                    }
                }
                HelperRecord::Post(post) => {
                    board.next = board.next.max(post.id.0);
                    match board.posts.iter().position(|p| p.id == post.id) {
                        Some(at) => board.posts[at] = *post,
                        None => board.posts.push(*post),
                    }
                }
                // Routines are the routine book's, read back by their own query.
                HelperRecord::Routine(_) => {}
            }
        }
        for card in &mut board.cards {
            if matches!(
                card.state,
                HelperState::Working(_) | HelperState::Waiting(_)
            ) {
                card.state = HelperState::Idle;
            }
        }
        for job in &mut board.jobs {
            match job.status {
                JobStatus::Taken(_) => {
                    job.status = JobStatus::Open;
                    job.failed = Some(JobFault::Crashed);
                }
                JobStatus::Blocked(_) => job.status = JobStatus::Open,
                JobStatus::Open | JobStatus::Done(_) => {}
            }
        }
        let known: Vec<HelperId> = board.cards.iter().map(|c| c.id.clone()).collect();
        board.jobs.retain(|j| known.contains(&j.owner));
        board.posts.retain(|p| known.contains(&p.from));
        board
    }

    /// Every card, job and post as a record, cards first: the board written again from scratch,
    /// so what a restart reads has the newest of each at its head.
    pub fn records(&self) -> Vec<HelperRecord> {
        let cards = self
            .cards
            .iter()
            .map(|c| HelperRecord::Card(CardRecord::Kept(Box::new(c.clone()))));
        let jobs = self
            .jobs
            .iter()
            .map(|j| HelperRecord::Job(Box::new(j.clone())));
        let posts = self
            .posts
            .iter()
            .map(|p| HelperRecord::Post(Box::new(p.clone())));
        cards.chain(jobs).chain(posts).collect()
    }

    /// Every card, oldest first.
    pub fn cards(&self) -> &[HelperCard] {
        &self.cards
    }

    /// One card.
    pub fn card(&self, id: &HelperId) -> Option<&HelperCard> {
        self.cards.iter().find(|c| &c.id == id)
    }

    /// The card the person calls `name` (letter case does not matter).
    pub fn by_name(&self, name: &str) -> Option<&HelperCard> {
        self.cards
            .iter()
            .find(|c| c.name.as_str().eq_ignore_ascii_case(name.trim()))
    }

    /// Every job, oldest first.
    pub fn jobs(&self) -> &[Job] {
        &self.jobs
    }

    /// One job.
    pub fn job(&self, id: JobId) -> Option<&Job> {
        self.jobs.iter().find(|j| j.id == id)
    }

    /// Every post, oldest first.
    pub fn posts(&self) -> &[Post] {
        &self.posts
    }

    /// The live job that owns `thing`, and its helper, not counting the job `except`. Only one
    /// job writes a thing at a time.
    pub fn owner_of(&self, thing: &EntityId, except: Option<JobId>) -> Option<(&HelperCard, &Job)> {
        self.jobs
            .iter()
            .filter(|j| j.is_live() && Some(j.id) != except && j.owns.contains(thing))
            .find_map(|j| self.card(&j.owner).map(|card| (card, j)))
    }

    /// The posts `helper` should be handed at the start of a job.
    pub fn pending_posts(&self, helper: &HelperId) -> Vec<&Post> {
        self.posts.iter().filter(|p| p.waits_for(helper)).collect()
    }

    /// The job a helper is working on now.
    pub fn running(&self, helper: &HelperId) -> Option<&Job> {
        self.jobs
            .iter()
            .find(|j| j.status == JobStatus::Taken(helper.clone()))
    }
}

/// The board after `event`, and what the host does about it.
pub fn step(mut board: Board, event: BoardEvent) -> (Board, Vec<BoardEffect>) {
    let mut fx = Vec::new();
    match event {
        BoardEvent::Create(draft) => board.create(*draft, &mut fx),
        BoardEvent::Edit { helper, draft } => board.edit(&helper, *draft, &mut fx),
        BoardEvent::Pause(helper) => board.pause(&helper, PauseWhy::ByPerson, &mut fx),
        BoardEvent::Resume(helper) => board.resume(&helper, &mut fx),
        BoardEvent::Remove(helper) => board.remove(&helper, &mut fx),
        BoardEvent::Post(new) => board.post_job(*new, RunTerms::default(), &mut fx),
        BoardEvent::PostRun { job, about, steps } => board.post_job(
            *job,
            RunTerms {
                about,
                steps: Some(steps),
            },
            &mut fx,
        ),
        BoardEvent::Take(helper) => board.launch_next(&helper, Retry::Fresh, &mut fx),
        BoardEvent::Retry(helper) => board.launch_next(&helper, Retry::Again, &mut fx),
        BoardEvent::Started { job, task } => board.started(job, task, &mut fx),
        BoardEvent::Finished { job, result } => board.finished(job, result, &mut fx),
        BoardEvent::Failed { job, fault } => board.failed(job, fault, &mut fx),
        BoardEvent::NeedsSignIn(job) => board.needs_sign_in(job, &mut fx),
        BoardEvent::SignedIn(helper) => board.signed_in(&helper, &mut fx),
        BoardEvent::Stopped(job) => board.stopped(job, &mut fx),
        BoardEvent::Called { helper, verdict } => board.called(&helper, verdict, &mut fx),
        BoardEvent::Hand {
            from,
            to,
            parts,
            label,
        } => board.hand(from, to, parts, label, &mut fx),
        BoardEvent::Delivered { post, helper } => board.delivered(post, helper, &mut fx),
    }
    (board, fx)
}
