//! The board: every helper's card, every job and every post, moved by one pure function.
//! companiond owns the board, so taking a job is decided here and needs no compare-and-set; it
//! carries out the effects `step` returns (writing the records, starting the task, telling the
//! person) and feeds back what happened.

use super::card::TaskRef;
use super::card::{CardDraft, HelperCard, HelperModel, HelperName, HelperState, Need, PauseWhy};
use super::ids::{helper_id, helper_number};
use super::job::{GoalOrigin, GoalText, Job, JobFault, JobId, JobStatus, ResultHandle};
use super::notice::Notice;
use super::post::{Post, PostId, PostPart, PostTo};
use super::records::{CardRecord, HelperRecord};
use super::rules::{JobRefusal, admit_goal};
use almanac_core::EventRef;
use prov::{EntityId, HelperId, Label, TaskId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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
    /// A job is posted for a routine's run that an almanac event started: the event goes with the
    /// job as a handle, and no part of it is in the goal.
    PostAbout {
        /// The job.
        job: Box<NewJob>,
        /// The event.
        about: EventRef,
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

    fn mint(&mut self) -> u64 {
        self.next = self.next.saturating_add(1);
        self.next
    }

    fn card_mut(&mut self, id: &HelperId) -> Option<&mut HelperCard> {
        self.cards.iter_mut().find(|c| &c.id == id)
    }

    fn job_mut(&mut self, id: JobId) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|j| j.id == id)
    }

    fn streak(&mut self, helper: &HelperId) -> &mut Streak {
        let at = match self.streaks.iter().position(|s| &s.helper == helper) {
            Some(at) => at,
            None => {
                self.streaks.push(Streak {
                    helper: helper.clone(),
                    denied: 0,
                    failed: 0,
                });
                self.streaks.len() - 1
            }
        };
        &mut self.streaks[at]
    }

    fn say(&self, helper: &HelperId, notice: Notice, fx: &mut Vec<BoardEffect>) {
        if let Some(card) = self.card(helper) {
            fx.push(BoardEffect::Say {
                helper: helper.clone(),
                name: card.name.clone(),
                notice,
            });
        }
    }

    fn create(&mut self, draft: CardDraft, fx: &mut Vec<BoardEffect>) {
        let n = self.mint();
        let Some(id) = helper_id(n) else {
            return;
        };
        let card = HelperCard::new(id.clone(), draft);
        fx.push(save_card(&card));
        self.cards.push(card);
        fx.push(BoardEffect::Created(id));
    }

    fn edit(&mut self, helper: &HelperId, draft: CardDraft, fx: &mut Vec<BoardEffect>) {
        let Some(card) = self.card_mut(helper) else {
            return;
        };
        card.name = draft.name;
        card.role = draft.role;
        card.model = draft.model;
        card.reach = draft.reach;
        card.budget = draft.budget;
        let chose = matches!(card.model, HelperModel::Named(_))
            && card.state == HelperState::Waiting(Need::ChooseModel);
        if chose {
            card.state = HelperState::Idle;
        }
        fx.push(save_card(card));
        if chose {
            for job in self
                .jobs
                .iter_mut()
                .filter(|j| &j.owner == helper && j.status == JobStatus::Blocked(Need::ChooseModel))
            {
                job.status = JobStatus::Open;
                fx.push(save_job(job));
            }
        }
        self.launch_next(helper, Retry::Fresh, fx);
    }

    /// Gives `helper` its oldest open job, if it is free to take one. A helper with no model
    /// takes none: its job is blocked on the choice and the person is told once; nothing stands
    /// in for the model they have not chosen.
    fn launch_next(&mut self, helper: &HelperId, retry: Retry, fx: &mut Vec<BoardEffect>) {
        let Some(card) = self.card(helper) else {
            return;
        };
        if !card.state.takes_jobs() || self.running(helper).is_some() {
            return;
        }
        // A helper that waits for the person to sign in takes nothing until they have, unless
        // they ask it to try again.
        if card.state == HelperState::Waiting(Need::SignIn) && retry == Retry::Fresh {
            return;
        }
        // An agent app brings its own model.
        let model_unset = card.model == HelperModel::Unset && card.kind.program().is_none();
        let Some(at) = self.jobs.iter().position(|j| {
            &j.owner == helper
                && matches!(j.status, JobStatus::Open | JobStatus::Blocked(_))
                && (retry == Retry::Again || j.failed.is_none())
        }) else {
            return;
        };
        if model_unset {
            self.jobs[at].status = JobStatus::Blocked(Need::ChooseModel);
            fx.push(save_job(&self.jobs[at]));
            let first_time = match self.card_mut(helper) {
                Some(card) if card.state != HelperState::Waiting(Need::ChooseModel) => {
                    card.state = HelperState::Waiting(Need::ChooseModel);
                    fx.push(save_card(card));
                    true
                }
                _ => false,
            };
            if first_time {
                self.say(helper, Notice::WaitsForModel, fx);
            }
            return;
        }
        self.jobs[at].status = JobStatus::Taken(helper.clone());
        let job = self.jobs[at].id;
        fx.push(save_job(&self.jobs[at]));
        if let Some(card) = self.card_mut(helper)
            && matches!(card.state, HelperState::Waiting(_))
        {
            card.state = HelperState::Idle;
            fx.push(save_card(card));
        }
        fx.push(BoardEffect::Launch(Launch {
            helper: helper.clone(),
            job,
        }));
    }

    fn post_job(&mut self, new: NewJob, about: Option<EventRef>, fx: &mut Vec<BoardEffect>) {
        let source = match admit_goal(new.origin) {
            Ok(source) => source,
            Err(refusal) => {
                fx.push(BoardEffect::JobRefused(refusal));
                return;
            }
        };
        if self.card(&new.to).is_none() {
            fx.push(BoardEffect::JobRefused(JobRefusal::NoSuchHelper));
            return;
        }
        for thing in &new.owns {
            if let Some((owner, _)) = self.owner_of(thing, None) {
                fx.push(BoardEffect::JobRefused(JobRefusal::Owned(
                    owner.name.clone(),
                )));
                return;
            }
        }
        let with: Vec<JobId> = new
            .with
            .into_iter()
            .filter(|w| matches!(self.job(*w).map(|j| &j.status), Some(JobStatus::Done(_))))
            .collect();
        let job = Job {
            id: JobId(self.mint()),
            goal: new.goal,
            source,
            owner: new.to.clone(),
            status: JobStatus::Open,
            owns: new.owns.into_iter().collect::<BTreeSet<_>>(),
            giver: new.giver,
            with,
            failed: None,
            about,
        };
        fx.push(save_job(&job));
        fx.push(BoardEffect::Posted(job.id));
        self.jobs.push(job);
        self.launch_next(&new.to, Retry::Fresh, fx);
    }

    /// Ends the helper's hold on `job` (it is no longer working on it), leaving a pause alone.
    fn free(&mut self, job: JobId, fx: &mut Vec<BoardEffect>) {
        if let Some(card) = self
            .cards
            .iter_mut()
            .find(|c| c.working_on().is_some_and(|at| at.job == job))
        {
            card.state = HelperState::Idle;
            fx.push(save_card(card));
        }
    }

    fn started(&mut self, job: JobId, task: TaskId, fx: &mut Vec<BoardEffect>) {
        let Some(owner) = self.job(job).and_then(|j| match &j.status {
            JobStatus::Taken(helper) => Some(helper.clone()),
            _ => None,
        }) else {
            return;
        };
        if let Some(card) = self.card_mut(&owner)
            && !matches!(card.state, HelperState::Paused(_))
        {
            card.state = HelperState::Working(TaskRef { job, task });
            fx.push(save_card(card));
        }
    }

    fn finished(&mut self, job: JobId, result: ResultHandle, fx: &mut Vec<BoardEffect>) {
        let Some(found) = self.job_mut(job) else {
            return;
        };
        let owner = found.owner.clone();
        found.status = JobStatus::Done(result);
        found.failed = None;
        fx.push(save_job(found));
        self.free(job, fx);
        self.streak(&owner).failed = 0;
        self.launch_next(&owner, Retry::Fresh, fx);
    }

    fn failed(&mut self, job: JobId, fault: JobFault, fx: &mut Vec<BoardEffect>) {
        let Some(found) = self.job_mut(job) else {
            return;
        };
        let owner = found.owner.clone();
        found.status = JobStatus::Open;
        found.failed = Some(fault);
        fx.push(save_job(found));
        self.free(job, fx);
        self.say(&owner, Notice::Failed(fault), fx);
        let streak = self.streak(&owner);
        streak.failed = streak.failed.saturating_add(1);
        if streak.failed >= IN_A_ROW {
            streak.failed = 0;
            self.pause(&owner, PauseWhy::Failed3, fx);
        }
    }

    /// The job waits for the person to sign in, and so does its helper. The person is told once.
    fn needs_sign_in(&mut self, job: JobId, fx: &mut Vec<BoardEffect>) {
        let Some(found) = self.job_mut(job) else {
            return;
        };
        let owner = found.owner.clone();
        found.status = JobStatus::Blocked(Need::SignIn);
        fx.push(save_job(found));
        let first_time = match self.card_mut(&owner) {
            Some(card) if !matches!(card.state, HelperState::Paused(_)) => {
                let first = card.state != HelperState::Waiting(Need::SignIn);
                card.state = HelperState::Waiting(Need::SignIn);
                fx.push(save_card(card));
                first
            }
            _ => false,
        };
        if first_time {
            self.say(&owner, Notice::WaitsForSignIn, fx);
        }
    }

    /// The helper's agent app is signed in: its blocked jobs are open, and it takes the oldest.
    fn signed_in(&mut self, helper: &HelperId, fx: &mut Vec<BoardEffect>) {
        let Some(card) = self.card_mut(helper) else {
            return;
        };
        if card.state == HelperState::Waiting(Need::SignIn) {
            card.state = HelperState::Idle;
            fx.push(save_card(card));
        }
        for job in self
            .jobs
            .iter_mut()
            .filter(|j| &j.owner == helper && j.status == JobStatus::Blocked(Need::SignIn))
        {
            job.status = JobStatus::Open;
            fx.push(save_job(job));
        }
        self.launch_next(helper, Retry::Fresh, fx);
    }

    fn stopped(&mut self, job: JobId, fx: &mut Vec<BoardEffect>) {
        if let Some(found) = self.job_mut(job)
            && matches!(found.status, JobStatus::Taken(_))
        {
            found.status = JobStatus::Open;
            fx.push(save_job(found));
        }
        self.free(job, fx);
    }

    fn called(&mut self, helper: &HelperId, verdict: CallVerdict, fx: &mut Vec<BoardEffect>) {
        let streak = self.streak(helper);
        match verdict {
            CallVerdict::Allowed => streak.denied = 0,
            CallVerdict::Denied => {
                streak.denied = streak.denied.saturating_add(1);
                if streak.denied >= IN_A_ROW {
                    streak.denied = 0;
                    self.pause(helper, PauseWhy::Denied3, fx);
                }
            }
        }
    }

    /// Stops a helper taking jobs, and stops the task it is running: that job is open again and
    /// nothing is held against it. The person is told once, unless they did it themselves.
    fn pause(&mut self, helper: &HelperId, why: PauseWhy, fx: &mut Vec<BoardEffect>) {
        let Some(card) = self.card_mut(helper) else {
            return;
        };
        if matches!(card.state, HelperState::Paused(_)) {
            return;
        }
        let running = card.working_on().cloned();
        card.state = HelperState::Paused(why);
        fx.push(save_card(card));
        if let Some(at) = running {
            if let Some(job) = self.job_mut(at.job)
                && matches!(job.status, JobStatus::Taken(_))
            {
                job.status = JobStatus::Open;
                fx.push(save_job(job));
            }
            fx.push(BoardEffect::Stop(at));
        }
        if why != PauseWhy::ByPerson {
            self.say(helper, Notice::Paused(why), fx);
        }
    }

    fn resume(&mut self, helper: &HelperId, fx: &mut Vec<BoardEffect>) {
        let Some(card) = self.card_mut(helper) else {
            return;
        };
        if !matches!(card.state, HelperState::Paused(_)) {
            return;
        }
        card.state = HelperState::Idle;
        fx.push(save_card(card));
        let streak = self.streak(helper);
        streak.denied = 0;
        streak.failed = 0;
        self.launch_next(helper, Retry::Again, fx);
    }

    fn remove(&mut self, helper: &HelperId, fx: &mut Vec<BoardEffect>) {
        let Some(card) = self.card(helper) else {
            return;
        };
        if let Some(at) = card.working_on().cloned() {
            fx.push(BoardEffect::Stop(at));
        }
        self.cards.retain(|c| &c.id != helper);
        self.jobs.retain(|j| &j.owner != helper);
        self.posts
            .retain(|p| &p.from != helper && p.to != PostTo::Helper(helper.clone()));
        self.streaks.retain(|s| &s.helper != helper);
        fx.push(BoardEffect::Save(HelperRecord::Card(CardRecord::Removed(
            helper.clone(),
        ))));
    }

    fn hand(
        &mut self,
        from: HelperId,
        to: PostTo,
        parts: Vec<PostPart>,
        label: Label,
        fx: &mut Vec<BoardEffect>,
    ) {
        if self.card(&from).is_none() {
            return;
        }
        let post = Post {
            id: PostId(self.mint()),
            from,
            to,
            parts,
            label,
            seen_by: Vec::new(),
        };
        fx.push(BoardEffect::Save(HelperRecord::Post(Box::new(
            post.clone(),
        ))));
        self.posts.push(post);
    }

    fn delivered(&mut self, post: PostId, helper: HelperId, fx: &mut Vec<BoardEffect>) {
        if let Some(found) = self.posts.iter_mut().find(|p| p.id == post)
            && !found.seen_by.contains(&helper)
        {
            found.seen_by.push(helper);
            fx.push(BoardEffect::Save(HelperRecord::Post(Box::new(
                found.clone(),
            ))));
        }
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
        BoardEvent::Post(new) => board.post_job(*new, None, &mut fx),
        BoardEvent::PostAbout { job, about } => board.post_job(*job, Some(about), &mut fx),
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
