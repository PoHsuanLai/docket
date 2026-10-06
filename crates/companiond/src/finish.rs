//! The end of a task: its trusted episode, its report to whoever spawned it, its session closed,
//! and its narrative queued for the idle pass. The skeleton is built without a model from what
//! the task did (the person's words, typed steps, entity ids); the narrative is written later,
//! when the person is away, and never steers anything by itself.

use crate::fault::ServeFault;
use crate::linger::{LINGER, finished};
use crate::runtime::{Companiond, Narration, REMEMBERED};
use agent_loop::{
    EpisodeJob, FinishedAs, FrontEvent, IdleInput, LoopPhase, ReadUntrusted, front_step,
};
use almanac_core::{Episode, EpisodeId, EpisodeKind, EpisodeOutcome};
use docket_client::Transport as IntentsTransport;
use docket_core::{DraftPart, EpisodeLine, NoteAsk, SkeletonText, TaskLedger, close};
use porter_client::Transport as InferTransport;
use prov::{Address, AgentRef, MessageKind, MessageText, ReportStatus, TaskId};

fn outcome_of(how: FinishedAs) -> EpisodeOutcome {
    match how {
        FinishedAs::Done => EpisodeOutcome::Done,
        FinishedAs::Failed => EpisodeOutcome::Failed,
        FinishedAs::Cancelled => EpisodeOutcome::Cancelled,
    }
}

fn status_of(how: FinishedAs) -> ReportStatus {
    match how {
        FinishedAs::Done => ReportStatus::Done,
        FinishedAs::Failed => ReportStatus::Failed,
        FinishedAs::Cancelled => ReportStatus::Cancelled,
    }
}

impl<P: InferTransport, I: IntentsTransport> Companiond<P, I> {
    /// The episode a task leaves: what it was asked, the typed steps it took, and how it ended.
    fn episode_of(&self, task: &TaskId, how: FinishedAs) -> Option<Episode> {
        let rt = self.runtimes.get(task)?;
        let ledger = TaskLedger {
            task: task.clone(),
            agent: rt.agent.clone(),
            parent: rt
                .parent
                .as_ref()
                .and_then(|p| EpisodeId::parse(p.as_str()).ok()),
            space: rt.space.clone(),
            started: rt.started,
            asked: rt.turns.clone(),
            steps: rt.steps.clone(),
            touched: vec![],
            results: vec![],
        };
        close(
            &ledger,
            EpisodeKind::Task,
            self.clock.now(),
            outcome_of(how),
        )
    }

    /// Ends `task`: called when its loop finishes.
    pub(crate) async fn finish(&mut self, task: &TaskId) -> Result<(), ServeFault> {
        let how = match self.tasks.get(task).map(|s| s.phase) {
            Some(LoopPhase::Finished(how)) => how,
            _ => FinishedAs::Done,
        };
        let episode = self.episode_of(task, how);
        let Some(rt) = self.runtimes.get(task).cloned() else {
            return Err(ServeFault::UnknownSession);
        };
        // A worker answers whoever spawned it, in the one message model: a final report in the
        // thread of the request that gave it its goal. The router moves its task to ended on it.
        if matches!(rt.agent, AgentRef::Worker { .. }) {
            self.report(task, how).await;
        }
        // The router leaves the episode itself: when the task ends here (`NoteAsk::End`), or when
        // the final report ends it first. The companion keeps its own copy only for the planner's
        // section. The session stays open so the screen can still show the answer's handles
        // (`Session.Display`); `dismiss` closes it, or the oldest finished one is closed when more
        // than `LINGER` wait.
        let record = companion_wire::SessionRecord::Finished {
            task: task.clone(),
            phase: rt.phase.clone(),
        };
        self.record(&rt.session, &record).await;
        let _ = self
            .intents
            .session_note(rt.session.clone(), NoteAsk::End)
            .await;
        if let Some(episode) = episode {
            self.remember(task, &rt.session, episode);
        }
        self.front = front_step(self.front.take(), &FrontEvent::Ended(task.clone()));
        self.publish();
        let evicted = finished(std::mem::take(&mut self.lingering), task.clone(), LINGER);
        self.lingering = evicted.queue;
        for old in evicted.close {
            if let Some(session) = self.runtimes.get(&old).map(|r| r.session.clone()) {
                self.record(&session, &companion_wire::SessionRecord::Closed)
                    .await;
            }
            self.dismiss(&old).await;
        }
        Ok(())
    }

    /// The worker's final report to its parent: how it ended, with the typed counts and nothing
    /// the worker read.
    async fn report(&mut self, task: &TaskId, how: FinishedAs) -> bool {
        let Some(rt) = self.runtimes.get(task) else {
            return false;
        };
        let Some(parent) = rt
            .parent
            .as_ref()
            .and_then(|p| self.runtimes.get(p))
            .map(|p| Address::new(p.agent.clone(), p.space.clone()))
        else {
            return false;
        };
        let (thread, goal) = rt
            .inbox
            .first()
            .map(|m| (Some(m.thread.clone()), Some(m.id.clone())))
            .unwrap_or_default();
        let words = format!("{} steps taken", rt.steps.len());
        let draft = docket_core::MessageDraft {
            to: parent,
            thread,
            in_reply_to: goal,
            kind: MessageKind::Report {
                status: status_of(how),
            },
            parts: vec![DraftPart::Text(MessageText::new(words))],
        };
        let session = rt.session.clone();
        self.intents.send(session, draft).await.is_ok()
    }

    /// Keeps the episode for the recent-episodes section and queues its narrative.
    fn remember(&mut self, task: &TaskId, session: &prov::SessionId, episode: Episode) {
        let line = EpisodeLine {
            id: episode.id.clone(),
            agent: episode.agent.clone(),
            space: episode.space.clone(),
            ended: episode.ended,
            outcome: episode.outcome.clone(),
            skeleton: SkeletonText(episode.skeleton.text()),
            narrative: None,
        };
        self.episodes.insert(0, line);
        self.episodes.truncate(REMEMBERED * 2);
        let read = self
            .runtimes
            .get(task)
            .map_or(ReadUntrusted::No, |rt| match rt.taint() {
                prov::Integrity::Trusted => ReadUntrusted::No,
                prov::Integrity::Untrusted => ReadUntrusted::Yes,
            });
        self.idle_apply(IdleInput::EpisodeClosed(EpisodeJob {
            episode: episode.id.clone(),
            read,
        }));
        self.narration.insert(
            episode.id.clone(),
            Narration {
                episode,
                session: session.clone(),
            },
        );
    }
}
