//! The board's moves: each event, one private method on [`Board`] that changes the board and
//! pushes the effects the host carries out.

use super::{
    Board, BoardEffect, CallVerdict, IN_A_ROW, Launch, NewJob, Retry, RunTerms, Streak, save_card,
    save_job,
};
use crate::helper::card::{
    CardDraft, HelperCard, HelperModel, HelperState, Need, PauseWhy, TaskRef,
};
use crate::helper::ids::helper_id;
use crate::helper::job::{Job, JobFault, JobId, JobStatus, ResultHandle};
use crate::helper::notice::Notice;
use crate::helper::post::{Post, PostId, PostPart, PostTo};
use crate::helper::records::{CardRecord, HelperRecord};
use crate::helper::rules::{JobRefusal, admit_goal};
use prov::{HelperId, Label, TaskId};
use std::collections::BTreeSet;

impl Board {
    pub(super) fn mint(&mut self) -> u64 {
        self.next = self.next.saturating_add(1);
        self.next
    }

    pub(super) fn card_mut(&mut self, id: &HelperId) -> Option<&mut HelperCard> {
        self.cards.iter_mut().find(|c| &c.id == id)
    }

    pub(super) fn job_mut(&mut self, id: JobId) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|j| j.id == id)
    }

    pub(super) fn streak(&mut self, helper: &HelperId) -> &mut Streak {
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

    pub(super) fn say(&self, helper: &HelperId, notice: Notice, fx: &mut Vec<BoardEffect>) {
        if let Some(card) = self.card(helper) {
            fx.push(BoardEffect::Say {
                helper: helper.clone(),
                name: card.name.clone(),
                notice,
            });
        }
    }

    pub(super) fn create(&mut self, draft: CardDraft, fx: &mut Vec<BoardEffect>) {
        let n = self.mint();
        let Some(id) = helper_id(n) else {
            return;
        };
        let card = HelperCard::new(id.clone(), draft);
        fx.push(save_card(&card));
        self.cards.push(card);
        fx.push(BoardEffect::Created(id));
    }

    pub(super) fn edit(&mut self, helper: &HelperId, draft: CardDraft, fx: &mut Vec<BoardEffect>) {
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
    pub(super) fn launch_next(
        &mut self,
        helper: &HelperId,
        retry: Retry,
        fx: &mut Vec<BoardEffect>,
    ) {
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

    pub(super) fn post_job(&mut self, new: NewJob, run: RunTerms, fx: &mut Vec<BoardEffect>) {
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
            about: run.about,
            steps: run.steps,
        };
        fx.push(save_job(&job));
        fx.push(BoardEffect::Posted(job.id));
        self.jobs.push(job);
        self.launch_next(&new.to, Retry::Fresh, fx);
    }

    /// Ends the helper's hold on `job` (it is no longer working on it), leaving a pause alone.
    pub(super) fn free(&mut self, job: JobId, fx: &mut Vec<BoardEffect>) {
        if let Some(card) = self
            .cards
            .iter_mut()
            .find(|c| c.working_on().is_some_and(|at| at.job == job))
        {
            card.state = HelperState::Idle;
            fx.push(save_card(card));
        }
    }

    pub(super) fn started(&mut self, job: JobId, task: TaskId, fx: &mut Vec<BoardEffect>) {
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

    pub(super) fn finished(&mut self, job: JobId, result: ResultHandle, fx: &mut Vec<BoardEffect>) {
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

    pub(super) fn failed(&mut self, job: JobId, fault: JobFault, fx: &mut Vec<BoardEffect>) {
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
    pub(super) fn needs_sign_in(&mut self, job: JobId, fx: &mut Vec<BoardEffect>) {
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
    pub(super) fn signed_in(&mut self, helper: &HelperId, fx: &mut Vec<BoardEffect>) {
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

    pub(super) fn stopped(&mut self, job: JobId, fx: &mut Vec<BoardEffect>) {
        if let Some(found) = self.job_mut(job)
            && matches!(found.status, JobStatus::Taken(_))
        {
            found.status = JobStatus::Open;
            fx.push(save_job(found));
        }
        self.free(job, fx);
    }

    pub(super) fn called(
        &mut self,
        helper: &HelperId,
        verdict: CallVerdict,
        fx: &mut Vec<BoardEffect>,
    ) {
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
    pub(super) fn pause(&mut self, helper: &HelperId, why: PauseWhy, fx: &mut Vec<BoardEffect>) {
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

    pub(super) fn resume(&mut self, helper: &HelperId, fx: &mut Vec<BoardEffect>) {
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

    pub(super) fn remove(&mut self, helper: &HelperId, fx: &mut Vec<BoardEffect>) {
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

    pub(super) fn hand(
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

    pub(super) fn delivered(&mut self, post: PostId, helper: HelperId, fx: &mut Vec<BoardEffect>) {
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
