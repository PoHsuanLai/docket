//! The idle pass: the model-written narrative of each finished task, written only when the
//! person is not waiting. It runs on the one resident model as background work and yields to
//! any interactive request at once (the stream is cancelled and the job retried later), so it
//! never blocks a prompt. The narrative is reader-shaped on purpose: when the task read
//! untrusted content it runs in readerd (no tools), so the model that writes a narrative over
//! mail text can never act.

use almanac_core::EpisodeId;
use docket_core::IdleRules;
use prov::UnixSeconds;
use serde::{Deserialize, Serialize};

/// Whether a task read untrusted content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadUntrusted {
    /// It did.
    Yes,
    /// It did not.
    No,
}

/// Where the narrative runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NarrativeVia {
    /// In readerd: no tools.
    Reader,
    /// In companiond: the task was untainted.
    Companion,
}

/// One finished task waiting for its narrative.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeJob {
    /// The episode.
    pub episode: EpisodeId,
    /// Whether its task read untrusted content.
    pub read: ReadUntrusted,
}

/// A narrative being written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NarrativeJob {
    /// The episode.
    pub episode: EpisodeId,
    /// Where it runs.
    pub via: NarrativeVia,
}

/// Where the pass is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum IdlePhase {
    /// Something interactive is going on.
    Busy,
    /// Nothing interactive since then.
    Quiet {
        /// Since when.
        since: UnixSeconds,
    },
    /// A narrative is being written; the quiet that started it began at `since`, so the next
    /// one needs no new wait while the person is still away.
    Running {
        /// The narrative.
        job: NarrativeJob,
        /// When the quiet began.
        since: UnixSeconds,
    },
}

/// The pass: where it is, and what waits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdleState {
    /// Where it is.
    pub phase: IdlePhase,
    /// The tasks waiting for a narrative, oldest first.
    pub queue: Vec<EpisodeJob>,
}

impl IdleState {
    /// A pass that starts when nothing has been asked since `now`.
    pub fn quiet_since(now: UnixSeconds) -> Self {
        Self {
            phase: IdlePhase::Quiet { since: now },
            queue: vec![],
        }
    }
}

/// What happens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum IdleInput {
    /// An interactive request started (the launcher, a prompt).
    InteractiveStarted,
    /// The last interactive request ended.
    InteractiveEnded(UnixSeconds),
    /// A task closed and left its skeleton.
    EpisodeClosed(EpisodeJob),
    /// Time passed.
    Tick(UnixSeconds),
    /// The narrative was written.
    NarrativeDone(EpisodeId),
    /// It failed; it is tried again later.
    NarrativeFailed(EpisodeId),
}

/// What companiond does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum IdleEffect {
    /// Write the narrative as background work.
    Start(NarrativeJob),
    /// Cancel the stream now: an interactive request wants the model.
    Cancel(EpisodeId),
    /// Send the fact candidates of the narrative to `Propose` (untrusted ones land pending).
    ProposeFacts(EpisodeId),
}

fn via(read: ReadUntrusted) -> NarrativeVia {
    match read {
        ReadUntrusted::Yes => NarrativeVia::Reader,
        ReadUntrusted::No => NarrativeVia::Companion,
    }
}

/// One transition. A narrative starts only after `idle_after` of quiet; anything interactive
/// cancels a running one and puts its job back at the front of the queue.
pub fn idle_step(
    state: IdleState,
    input: IdleInput,
    rules: &IdleRules,
) -> (IdleState, Vec<IdleEffect>) {
    let IdleState { phase, mut queue } = state;
    let mut effects = Vec::new();
    let phase = match (phase, input) {
        (IdlePhase::Running { job, .. }, IdleInput::InteractiveStarted) => {
            effects.push(IdleEffect::Cancel(job.episode.clone()));
            queue.insert(
                0,
                EpisodeJob {
                    episode: job.episode,
                    read: read_of(job.via),
                },
            );
            IdlePhase::Busy
        }
        (_, IdleInput::InteractiveStarted) => IdlePhase::Busy,
        (IdlePhase::Busy, IdleInput::InteractiveEnded(at)) => IdlePhase::Quiet { since: at },
        (phase, IdleInput::InteractiveEnded(_)) => phase,
        (phase, IdleInput::EpisodeClosed(job)) => {
            queue.push(job);
            phase
        }
        (IdlePhase::Quiet { since }, IdleInput::Tick(now))
            if now.0 - since.0 >= i64::from(rules.idle_after.0) && !queue.is_empty() =>
        {
            let next = queue.remove(0);
            let job = NarrativeJob {
                via: via(next.read),
                episode: next.episode,
            };
            effects.push(IdleEffect::Start(job.clone()));
            IdlePhase::Running { job, since }
        }
        (phase, IdleInput::Tick(_)) => phase,
        (IdlePhase::Running { job, since }, IdleInput::NarrativeDone(done))
            if job.episode == done =>
        {
            effects.push(IdleEffect::ProposeFacts(done));
            IdlePhase::Quiet { since }
        }
        // A failed narrative goes to the back and waits for the person to come and go again,
        // so a model that keeps failing is not retried in a loop.
        (IdlePhase::Running { job, .. }, IdleInput::NarrativeFailed(failed))
            if job.episode == failed =>
        {
            queue.push(EpisodeJob {
                episode: job.episode,
                read: read_of(job.via),
            });
            IdlePhase::Busy
        }
        (phase, IdleInput::NarrativeDone(_) | IdleInput::NarrativeFailed(_)) => phase,
    };
    (IdleState { phase, queue }, effects)
}

fn read_of(via: NarrativeVia) -> ReadUntrusted {
    match via {
        NarrativeVia::Reader => ReadUntrusted::Yes,
        NarrativeVia::Companion => ReadUntrusted::No,
    }
}
