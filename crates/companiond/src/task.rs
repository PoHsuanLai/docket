//! What companiond keeps of one task while it runs: its session, the person's words, the steps
//! and handles the planner is shown, the messages that landed, and the answer so far. It keeps
//! no content past the task: the episode it leaves is the trusted skeleton, and the router's
//! own ledger is the audit.

use crate::plan::Plan;
use agent_loop::LoopState;
use companion_wire::{AnswerBody, AnswerPhase, AnswerWire, FooterWire, NeedsYou, RefusalWire};
use docket_core::{
    CallEnd, CallId, CallRefusal, CallRequest, CharCount, ContextKeep, HandleCard, HandleShape,
    InboundLine, InboundPart, Keep, LedgerStep, Outcome, Reveal, StepEnd, StepLine, StepShown,
    TargetValue, Undoable, UserTurn, Value, WindowKey,
};
use porter_core::AppName;
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
    /// The calls of this turn, as the plan card shows them.
    pub plan: Plan,
    /// Why the task failed, if a refusal ended it.
    pub refused: Option<RefusalWire>,
    /// Who answered the planner, last.
    pub served: Option<ServedBy>,
    /// The next number a call of this task gets.
    pub next_call: u64,
    /// The app the person asked from (the context section reads its window); none for the
    /// launcher alone.
    pub summoned: Option<AppName>,
    /// The structured answer a draft or a proposal made, whose cards the person may act on.
    pub proposal: Option<AnswerBody>,
    /// The cards the person acted on since the last turn, as plan steps.
    pub acts: Plan,
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
            plan: Plan::default(),
            refused: None,
            served: None,
            next_call: 0,
            summoned: None,
            proposal: None,
            acts: Plan::default(),
        }
    }

    /// The card of the proposal named `id`.
    pub fn card(&self, id: &docket_core::CardActionId) -> Option<&companion_wire::CardWire> {
        match self.proposal.as_ref()? {
            AnswerBody::DraftReply { actions, .. } | AnswerBody::ProposedEvent { actions, .. } => {
                actions.iter().find(|c| &c.id == id)
            }
            _ => None,
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
        let mut undone = None;
        let (end, ended) = match result {
            Ok(outcome) => {
                let value = outcome.value.map(|v| self.reveal(v));
                undone = match outcome.undo {
                    Undoable::Journaled(row) => Some(row),
                    Undoable::Yes(_) | Undoable::No => None,
                };
                (
                    StepEnd::Done {
                        said: outcome.said,
                        value,
                        undo: undone,
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
            undo: undone,
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
                        // A placeholder until the router's own card (`Session.Handles`) replaces it.
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
        let card = match &self.phase {
            AnswerPhase::Thinking
            | AnswerPhase::Streaming
            | AnswerPhase::NeedsYou(NeedsYou::Confirm(_)) => self.plan.wire(),
            _ => None,
        };
        // A card the person acted on shows its step, whatever came of it.
        let card = self.acts.wire().or(card);
        let body = match (&self.phase, &self.refused, card) {
            (AnswerPhase::Failed, Some(refusal), _) => AnswerBody::Refused(refusal.clone()),
            (_, _, Some(plan)) => AnswerBody::Plan(plan),
            _ if self.proposal.is_some() => self
                .proposal
                .clone()
                .unwrap_or(AnswerBody::Text { lines: Vec::new() }),
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

#[cfg(test)]
mod tests {
    use super::*;
    use docket_core::{CallId, CallProgress, ConfirmId, LabelText};
    use prov::ActionName;

    fn runtime() -> TaskRuntime {
        TaskRuntime::new(
            SessionId::parse("s-1").expect("session"),
            SpaceId::desktop(),
            AgentRef::Companion,
            None,
            UnixSeconds(0),
        )
    }

    fn task() -> TaskId {
        TaskId::parse("t-1").expect("task")
    }

    fn begin(rt: &mut TaskRuntime) {
        rt.plan.begin(
            CallId(0),
            docket_core::ActionRef {
                app: porter_core::AppName::parse("org.quire.Mail").expect("app"),
                name: ActionName::parse("mail.message.forward").expect("name"),
            },
            LabelText::parse("Forward").expect("label"),
            Effect::Outbound,
        );
    }

    /// The answer's phase and body through a call that asks: Thinking (text), Streaming (the
    /// card, step pending), NeedsYou(Confirm) (the card), Streaming (step running), Done (text).
    #[test]
    fn a_call_that_asks_walks_thinking_streaming_needs_you_streaming_done() {
        let mut rt = runtime();
        let id = ConfirmId::parse("c-1").expect("id");
        let is_plan = |rt: &TaskRuntime| matches!(rt.answer(&task()).body, AnswerBody::Plan(_));

        assert_eq!(rt.answer(&task()).phase, AnswerPhase::Thinking);
        assert!(!is_plan(&rt));

        begin(&mut rt);
        rt.phase = AnswerPhase::Streaming;
        assert!(is_plan(&rt));

        rt.phase = crate::plan::phase_of(&CallProgress::Confirming(id.clone()));
        assert_eq!(
            rt.answer(&task()).phase,
            AnswerPhase::NeedsYou(NeedsYou::Confirm(id))
        );
        assert!(is_plan(&rt));

        rt.plan.progress(CallId(0), &CallProgress::Dispatched);
        rt.phase = crate::plan::phase_of(&CallProgress::Dispatched);
        assert_eq!(rt.answer(&task()).phase, AnswerPhase::Streaming);

        rt.phase = AnswerPhase::Done;
        assert_eq!(rt.answer(&task()).phase, AnswerPhase::Done);
        assert!(!is_plan(&rt), "a finished answer shows its words");
    }

    #[test]
    fn a_question_to_the_person_shows_the_question_not_the_card() {
        let mut rt = runtime();
        begin(&mut rt);
        rt.phase = AnswerPhase::NeedsYou(NeedsYou::Question {
            text: "Which one?".to_owned(),
            choices: vec![],
        });
        assert!(matches!(rt.answer(&task()).body, AnswerBody::Text { .. }));
    }
}
