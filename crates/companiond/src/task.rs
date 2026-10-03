//! What companiond keeps of one task while it runs: its session, the person's words, the steps
//! and handles the planner is shown, the messages that landed, and the answer so far. It keeps
//! no content past the task: the episode it leaves is the trusted skeleton, and the router's
//! own ledger is the audit.

use agent_loop::LoopState;
use companion_wire::{AnswerBody, AnswerPhase, AnswerWire, FooterWire, RefusalWire};
use docket_core::{
    CallEnd, CallId, CallRefusal, CallRequest, CharCount, ContextKeep, HandleCard, HandleShape,
    InboundLine, InboundPart, Keep, LedgerStep, Outcome, Reveal, StepEnd, StepLine, StepShown,
    TargetValue, UserTurn, Value, WindowKey,
};
use porter_infer::ServedBy;
use prov::{
    AgentRef, Effect, EntityId, Integrity, Labelled, SessionId, Source, SpaceId, TaskId,
    UnixSeconds,
};

/// One task, as companiond holds it.
#[derive(Debug, Clone)]
pub struct TaskRuntime {
    /// The router's session for it.
    pub session: SessionId,
    /// Its Space.
    pub space: SpaceId,
    /// Who it is for.
    pub agent: AgentRef,
    /// The task that spawned it.
    pub parent: Option<TaskId>,
    /// When it started.
    pub started: UnixSeconds,
    /// The person's words, verbatim.
    pub turns: Vec<UserTurn>,
    /// The chips of context the person kept with the latest turn.
    pub keep: ContextKeep,
    /// The window the person asked from, to anchor a confirmation.
    pub window: Option<WindowKey>,
    /// The calls and how they ended, oldest first.
    pub history: Vec<StepLine>,
    /// The same calls as the episode keeps them.
    pub steps: Vec<LedgerStep>,
    /// What the planner may name and not read.
    pub handles: Vec<HandleCard>,
    /// Messages that landed, oldest first.
    pub inbox: Vec<InboundLine>,
    /// What the planner said to the person, in order.
    pub said: Vec<String>,
    /// Where the answer stands.
    pub phase: AnswerPhase,
    /// Why the task failed, if a refusal ended it.
    pub refused: Option<RefusalWire>,
    /// Who answered the planner, last.
    pub served: Option<ServedBy>,
    /// The next number a call of this task gets.
    pub next_call: u64,
}

/// A keep that sent everything: what an answer's footer says until the turn says otherwise.
pub fn kept_all() -> ContextKeep {
    ContextKeep {
        query: Keep::Kept,
        results: Keep::Kept,
        selection: Keep::Kept,
        window: Keep::Kept,
    }
}

impl TaskRuntime {
    /// A task that has just opened.
    pub fn new(
        session: SessionId,
        space: SpaceId,
        agent: AgentRef,
        parent: Option<TaskId>,
        started: UnixSeconds,
    ) -> Self {
        Self {
            session,
            space,
            agent,
            parent,
            started,
            turns: Vec::new(),
            keep: kept_all(),
            window: None,
            history: Vec::new(),
            steps: Vec::new(),
            handles: Vec::new(),
            inbox: Vec::new(),
            said: Vec::new(),
            phase: AnswerPhase::Thinking,
            refused: None,
            served: None,
            next_call: 0,
        }
    }

    /// The integrity the planner works at: untrusted once it holds a handle or a message it
    /// cannot read (the router's own taint is the one that gates; this only labels the view).
    pub fn taint(&self) -> Integrity {
        let unread = self.inbox.iter().any(|m| {
            m.parts
                .iter()
                .any(|p| matches!(p, InboundPart::Text(Reveal::Handle(_))))
        });
        if self.handles.is_empty() && !unread {
            Integrity::Trusted
        } else {
            Integrity::Untrusted
        }
    }

    /// Records how a call ended: the line the planner is shown, the step the episode keeps, and
    /// the handle a text value became. Returns what the loop is told.
    pub fn record_call(
        &mut self,
        call: &CallRequest,
        effect: Effect,
        result: Result<Outcome, CallRefusal>,
    ) -> StepEnd {
        let id = CallId(self.next_call);
        self.next_call += 1;
        let (end, ended) = match result {
            Ok(outcome) => {
                let value = outcome.value.map(|v| self.reveal(v));
                (
                    StepEnd::Done {
                        said: outcome.said,
                        value,
                        undo: None,
                    },
                    CallEnd::Done,
                )
            }
            Err(CallRefusal::Unconfirmed(why)) => (
                StepEnd::Unconfirmed(why),
                CallEnd::Refused(CallRefusal::Unconfirmed(why)),
            ),
            Err(refusal) => {
                self.refused = Some(refusal_wire(&refusal));
                (StepEnd::Refused(refusal.clone()), CallEnd::Refused(refusal))
            }
        };
        self.history.push(StepLine {
            call: id,
            action: call.action.clone(),
            effect,
            end: end.clone(),
            shown: StepShown::Full,
        });
        self.steps.push(LedgerStep {
            call: id,
            action: call.action.name.clone(),
            targets: targets_of(call),
            effect,
            end: ended,
            undo: None,
        });
        end
    }

    fn reveal(&mut self, held: Labelled<Value>) -> Reveal<Value> {
        match held.value {
            Value::Handle(handle) => {
                let from = held
                    .label
                    .sources
                    .iter()
                    .next()
                    .cloned()
                    .unwrap_or(Source::User);
                if !self.handles.iter().any(|c| c.handle == handle) {
                    self.handles.push(HandleCard {
                        handle,
                        shape: HandleShape::Text,
                        from,
                        // The router does not say how long the text behind a handle is.
                        size: CharCount(0),
                    });
                }
                Reveal::Handle(handle)
            }
            plain => Reveal::Plain(plain),
        }
    }

    /// The answer as the bus shows it.
    pub fn answer(&self, task: &TaskId) -> AnswerWire {
        let body = match (&self.phase, &self.refused) {
            (AnswerPhase::Failed, Some(refusal)) => AnswerBody::Refused(refusal.clone()),
            _ => AnswerBody::Text {
                lines: self.said.iter().cloned().map(Reveal::Plain).collect(),
            },
        };
        AnswerWire {
            task: task.clone(),
            phase: self.phase.clone(),
            body,
            footer: FooterWire {
                served: self.served.iter().cloned().collect(),
                sources: Vec::new(),
                keep: self.keep,
            },
        }
    }
}

fn refusal_wire(refusal: &CallRefusal) -> RefusalWire {
    match refusal {
        CallRefusal::Denied(code) => RefusalWire::NotAllowed(*code),
        CallRefusal::OverBudget(kind) => RefusalWire::OverBudget(*kind),
        CallRefusal::Paused(_) | CallRefusal::Halted(_) => {
            RefusalWire::NotAllowed(docket_core::DenyCode::NeedsUser)
        }
        CallRefusal::App(_)
        | CallRefusal::Unconfirmed(_)
        | CallRefusal::NoSuchAction(_)
        | CallRefusal::BadArgs { .. }
        | CallRefusal::AppUnavailable(_)
        | CallRefusal::Timeout => RefusalWire::Failed(String::new()),
    }
}

/// The things a call acts on, for the episode's skeleton: its target and its entity arguments.
fn targets_of(call: &CallRequest) -> Vec<EntityId> {
    let mut targets = match &call.target {
        TargetValue::Entities(es) => es.clone(),
        TargetValue::Nothing | TargetValue::Text(_) | TargetValue::Files(_) => Vec::new(),
    };
    for arg in call.args.values() {
        match &arg.value {
            Value::Entity(e) => targets.push(e.clone()),
            Value::Entities(es) => targets.extend(es.iter().cloned()),
            _ => {}
        }
    }
    targets
}

/// The phase a task's roster line shows.
pub fn roster_state(state: Option<&LoopState>, phase: &AnswerPhase) -> docket_core::RosterState {
    use agent_loop::{FinishedAs, LoopPhase};
    use docket_core::RosterState;
    match state.map(|s| s.phase) {
        None => RosterState::Starting,
        Some(LoopPhase::Finished(FinishedAs::Done)) => RosterState::Done,
        Some(LoopPhase::Finished(FinishedAs::Failed)) => RosterState::Failed,
        Some(LoopPhase::Finished(FinishedAs::Cancelled)) => RosterState::Cancelled,
        Some(LoopPhase::Paused(_)) => RosterState::Paused,
        Some(
            LoopPhase::Idle
            | LoopPhase::Planning
            | LoopPhase::AwaitingCalls
            | LoopPhase::AwaitingReader,
        ) => match phase {
            AnswerPhase::NeedsYou(_) => RosterState::NeedsYou,
            _ => RosterState::Working,
        },
    }
}
