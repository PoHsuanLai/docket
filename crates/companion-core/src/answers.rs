//! What the board keeps of finished jobs while the process lives: their answers (so a later job
//! can be handed them), and the sentence on why a job stopped. Neither is written down.

use crate::conclude::{Answer, Hold};
use crate::job::JobInput;
use companion_model::helper::{Job, JobId};
use prov::SessionId;
use std::collections::{BTreeMap, VecDeque};

/// How many finished jobs keep their session open, so their answers can be handed on.
pub const KEPT_ANSWERS: usize = 16;

/// The answers kept, the oldest dropped first, and why jobs stopped (as many, the oldest first).
#[derive(Debug, Default)]
pub struct Answers {
    answers: BTreeMap<JobId, Answer>,
    kept: VecDeque<JobId>,
    /// Why a job stopped, as it was told, for its line on the card.
    whys: BTreeMap<JobId, String>,
}

impl Answers {
    /// Keeps a finished job's answer for the jobs that are handed it. The oldest answers beyond
    /// the most kept are dropped; the sessions the hub must close for them are returned.
    pub fn keep(&mut self, job: JobId, answer: Answer) -> Vec<SessionId> {
        self.answers.insert(job, answer);
        self.kept.push_back(job);
        let mut old = Vec::new();
        while self.kept.len() > KEPT_ANSWERS {
            if let Some(oldest) = self.kept.pop_front()
                && let Some(gone) = self.answers.remove(&oldest)
                && gone.hold == Hold::Close
            {
                old.push(gone.session);
            }
        }
        old
    }

    /// Keeps why a job stopped. As many as the answers kept: the oldest go first.
    pub fn note_why(&mut self, job: JobId, why: String) {
        self.whys.insert(job, why);
        while self.whys.len() > KEPT_ANSWERS {
            self.whys.pop_first();
        }
    }

    /// The words a finished job answered, while they are kept.
    pub fn words(&self, job: JobId) -> Option<&str> {
        self.answers.get(&job).map(|a| a.words.as_str())
    }

    /// Why a job stopped, while that is kept.
    pub fn why(&self, job: JobId) -> Option<&str> {
        self.whys.get(&job).map(String::as_str)
    }

    /// The answers `job` is handed that are still held.
    pub fn inputs_for(&self, job: &Job) -> Vec<JobInput> {
        job.with
            .iter()
            .filter_map(|earlier| self.answers.get(earlier))
            .map(|a| JobInput {
                from: a.session.clone(),
                text: a.words.clone(),
            })
            .collect()
    }
}
