//! Scripted stand-ins for the seams that ask a person or a model: each answers from a queue
//! (or a fixed mode) and records what it was asked, so a test can assert call counts.

use action_review::{ReviewReason, ReviewRequest, ReviewVerdict, Reviewer};
use almanac_core::{MemoryReply, MemoryRequest};
use docket_core::{
    ActionCard, ConfirmAnswer, ConfirmEnd, ConfirmId, ConfirmRequest, Confirmer, PolicyWriter,
    Reader, ReaderAsk, ReaderError, ReasonCode, ReasonText, ReviewError, Stage, TaskPolicy,
    UserTurn, Value,
};
use docket_router::{LinkFault, MemoryLink};
use prov::{Quarantined, SessionId, SpaceId, TaskId};
use std::collections::VecDeque;
use std::sync::Mutex;

fn locked<T: Clone>(m: &Mutex<T>) -> T {
    // A poisoned lock still holds the data a test wants to read.
    match m.lock() {
        Ok(g) => g.clone(),
        Err(p) => p.into_inner().clone(),
    }
}

fn with<T, R>(m: &Mutex<T>, f: impl FnOnce(&mut T) -> R) -> R {
    match m.lock() {
        Ok(mut g) => f(&mut g),
        Err(p) => f(&mut p.into_inner()),
    }
}

/// Answers confirmations from a queue and records every request and cancel.
#[derive(Debug, Default)]
pub struct ScriptedConfirmer {
    answers: Mutex<VecDeque<ConfirmAnswer>>,
    requests: Mutex<Vec<ConfirmRequest>>,
    cancelled: Mutex<Vec<ConfirmId>>,
}

impl ScriptedConfirmer {
    /// A confirmer that gives these answers in order, then dismisses.
    pub fn answering(answers: Vec<ConfirmAnswer>) -> Self {
        Self {
            answers: Mutex::new(answers.into()),
            ..Self::default()
        }
    }

    /// Every request shown.
    pub fn requests(&self) -> Vec<ConfirmRequest> {
        locked(&self.requests)
    }

    /// Forgets the queued answers and everything shown or withdrawn.
    pub fn clear(&self) {
        with(&self.answers, VecDeque::clear);
        with(&self.requests, Vec::clear);
        with(&self.cancelled, Vec::clear);
    }

    /// Every id withdrawn.
    pub fn cancelled(&self) -> Vec<ConfirmId> {
        locked(&self.cancelled)
    }
}

impl Confirmer for ScriptedConfirmer {
    async fn confirm(&self, request: ConfirmRequest) -> ConfirmAnswer {
        with(&self.requests, |r| r.push(request));
        with(&self.answers, VecDeque::pop_front)
            .unwrap_or(ConfirmAnswer::Ended(ConfirmEnd::Dismissed))
    }

    async fn cancel(&self, id: &ConfirmId) {
        with(&self.cancelled, |c| c.push(id.clone()));
    }
}

/// A seam that keeps a record of what it was asked can drop it between two cases.
pub trait Forget {
    /// Forgets what was recorded.
    fn forget(&self);
}

impl Forget for ScriptedReviewer {
    fn forget(&self) {
        self.clear();
    }
}

/// What a scripted reviewer does once its queue is empty (or instead of one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewMode {
    /// Every stage allows: a fully hijacked judge.
    AlwaysAllow,
    /// Every stage asks.
    AlwaysAsk,
    /// Every stage times out.
    Timeout,
    /// Every stage never answers: only a deadline ends it.
    Hang,
}

/// A reviewer with a queue of verdicts, a fallback mode, and a log of every call.
#[derive(Debug)]
pub struct ScriptedReviewer {
    queue: Mutex<VecDeque<Result<ReviewVerdict, ReviewError>>>,
    mode: ReviewMode,
    calls: Mutex<Vec<(Stage, ReviewRequest)>>,
}

impl ScriptedReviewer {
    fn new(queue: Vec<Result<ReviewVerdict, ReviewError>>, mode: ReviewMode) -> Self {
        Self {
            queue: Mutex::new(queue.into()),
            mode,
            calls: Mutex::new(vec![]),
        }
    }

    /// Allows every stage.
    pub fn always_allow() -> Self {
        Self::new(vec![], ReviewMode::AlwaysAllow)
    }

    /// Asks at every stage.
    pub fn always_ask() -> Self {
        Self::new(vec![], ReviewMode::AlwaysAsk)
    }

    /// Never answers: the router's deadline is what ends each stage.
    pub fn hanging() -> Self {
        Self::new(vec![], ReviewMode::Hang)
    }

    /// Times out at every stage.
    pub fn timing_out() -> Self {
        Self::new(vec![], ReviewMode::Timeout)
    }

    /// Gives these verdicts in order, then falls back to `mode`.
    pub fn queued(queue: Vec<Result<ReviewVerdict, ReviewError>>, mode: ReviewMode) -> Self {
        Self::new(queue, mode)
    }

    /// Forgets every review asked for.
    pub fn clear(&self) {
        with(&self.calls, Vec::clear);
    }

    /// How many reviews were asked for.
    pub fn call_count(&self) -> usize {
        with(&self.calls, |c| c.len())
    }

    /// Every (stage, request) asked for.
    pub fn calls(&self) -> Vec<(Stage, ReviewRequest)> {
        locked(&self.calls)
    }
}

impl Reviewer for ScriptedReviewer {
    async fn review(
        &self,
        stage: Stage,
        request: &ReviewRequest,
    ) -> Result<ReviewVerdict, ReviewError> {
        with(&self.calls, |c| c.push((stage, request.clone())));
        if let Some(next) = with(&self.queue, VecDeque::pop_front) {
            return next;
        }
        match self.mode {
            ReviewMode::AlwaysAllow => Ok(ReviewVerdict::Allow),
            ReviewMode::AlwaysAsk => Ok(ReviewVerdict::Ask {
                why: ReviewReason {
                    code: ReasonCode::Uncertain,
                    text: ReasonText("scripted".into()),
                },
            }),
            ReviewMode::Timeout => Err(ReviewError::Timeout),
            ReviewMode::Hang => std::future::pending().await,
        }
    }
}

/// Memory with scripted replies; every request is recorded.
#[derive(Debug, Default)]
pub struct FakeMemory {
    replies: Mutex<VecDeque<MemoryReply>>,
    requests: Mutex<Vec<MemoryRequest>>,
}

impl FakeMemory {
    /// Memory that answers with these replies in order, then `Ok`.
    pub fn answering(replies: Vec<MemoryReply>) -> Self {
        Self {
            replies: Mutex::new(replies.into()),
            requests: Mutex::new(vec![]),
        }
    }

    /// Every request asked.
    pub fn requests(&self) -> Vec<MemoryRequest> {
        locked(&self.requests)
    }
}

impl MemoryLink for FakeMemory {
    async fn ask(&self, request: MemoryRequest) -> Result<MemoryReply, LinkFault> {
        with(&self.requests, |r| r.push(request));
        Ok(with(&self.replies, VecDeque::pop_front).unwrap_or(MemoryReply::Ok))
    }
}

/// A policy writer that returns a scripted policy or error, and counts its calls.
#[derive(Debug)]
pub struct ScriptedWriter {
    result: Result<TaskPolicy, ReviewError>,
    calls: Mutex<Vec<(TaskId, usize)>>,
}

impl ScriptedWriter {
    /// A writer that always returns `result`.
    pub fn returning(result: Result<TaskPolicy, ReviewError>) -> Self {
        Self {
            result,
            calls: Mutex::new(vec![]),
        }
    }

    /// A writer that always fails: no policy, every non-read call is outside.
    pub fn failing() -> Self {
        Self::returning(Err(ReviewError::Unavailable))
    }

    /// The task and the number of turns of every call.
    pub fn calls(&self) -> Vec<(TaskId, usize)> {
        locked(&self.calls)
    }
}

impl PolicyWriter for ScriptedWriter {
    async fn derive(
        &self,
        task: &TaskId,
        turns: &[UserTurn],
        _catalogue: &[ActionCard],
        _space: &SpaceId,
    ) -> Result<TaskPolicy, ReviewError> {
        with(&self.calls, |c| c.push((task.clone(), turns.len())));
        self.result.clone()
    }
}

/// A reader that answers from a queue of values or errors.
#[derive(Debug, Default)]
pub struct ScriptedReader {
    answers: Mutex<VecDeque<Result<Value, ReaderError>>>,
    asks: Mutex<Vec<ReaderAsk>>,
    sessions: Mutex<Vec<SessionId>>,
}

impl ScriptedReader {
    /// A reader that gives these answers in order, then refuses.
    pub fn answering(answers: Vec<Result<Value, ReaderError>>) -> Self {
        Self {
            answers: Mutex::new(answers.into()),
            asks: Mutex::new(vec![]),
            sessions: Mutex::new(vec![]),
        }
    }

    /// The session of every ask received, in order.
    pub fn sessions(&self) -> Vec<SessionId> {
        locked(&self.sessions)
    }

    /// Every ask received.
    pub fn asks(&self) -> Vec<ReaderAsk> {
        locked(&self.asks)
    }
}

impl Reader for ScriptedReader {
    async fn extract(
        &self,
        session: &SessionId,
        ask: ReaderAsk,
        _inputs: Vec<Quarantined<String>>,
    ) -> Result<Value, ReaderError> {
        with(&self.sessions, |s| s.push(session.clone()));
        with(&self.asks, |a| a.push(ask));
        with(&self.answers, VecDeque::pop_front).unwrap_or(Err(ReaderError::Refused))
    }
}
