//! What the in-app agent keeps of the task it is running: the person's words, the steps and
//! handles the planner is shown, and what the planner said. No content outlives the task: the
//! router's own ledger is the audit.

use agent_loop::LoopState;
use docket_core::{
    CallId, CallRefusal, CharCount, HandleCard, HandleShape, Outcome, Reveal, StepEnd, StepLine,
    StepShown, Undoable, UserTurn, Value,
};
use prov::{Effect, Integrity, Labelled, SessionId, Source};

/// One open task.
#[derive(Debug, Clone)]
pub(crate) struct OpenTask {
    pub session: SessionId,
    pub state: LoopState,
    pub turns: Vec<UserTurn>,
    pub history: Vec<StepLine>,
    pub handles: Vec<HandleCard>,
    pub said: Vec<String>,
    next_call: u64,
}

impl OpenTask {
    pub fn new(session: SessionId) -> Self {
        Self {
            session,
            state: LoopState {
                phase: agent_loop::LoopPhase::Idle,
                turn: None,
                steps: 0,
                pending: vec![],
                guard: Default::default(),
            },
            turns: Vec::new(),
            history: Vec::new(),
            handles: Vec::new(),
            said: Vec::new(),
            next_call: 0,
        }
    }

    /// Untrusted once the planner holds a handle: the router's own taint is the one that gates,
    /// this only labels the view.
    pub fn taint(&self) -> Integrity {
        if self.handles.is_empty() {
            Integrity::Trusted
        } else {
            Integrity::Untrusted
        }
    }

    /// Records how a call ended as the line the planner is shown, and says it to the loop.
    pub fn record(
        &mut self,
        action: docket_core::ActionRef,
        effect: Effect,
        result: Result<Outcome, CallRefusal>,
    ) -> StepEnd {
        let id = CallId(self.next_call);
        self.next_call += 1;
        let end = match result {
            Ok(outcome) => StepEnd::Done {
                said: outcome.said,
                value: outcome.value.map(|v| self.reveal(v)),
                undo: match outcome.undo {
                    Undoable::Journaled(row) => Some(row),
                    Undoable::Yes(_) | Undoable::No => None,
                },
            },
            Err(CallRefusal::Unconfirmed(why)) => StepEnd::Unconfirmed(why),
            Err(refusal) => StepEnd::Refused(refusal),
        };
        self.history.push(StepLine {
            call: id,
            action,
            effect,
            end: end.clone(),
            shown: StepShown::Full,
        });
        end
    }

    /// A call the loop did not make because the planner had made it before and nothing had
    /// changed: a line in the history, so the next view says why. The router is not asked.
    pub fn hold(
        &mut self,
        call: &docket_core::CallRequest,
        effect: Effect,
        why: docket_core::Held,
    ) {
        let id = CallId(self.next_call);
        self.next_call += 1;
        self.history.push(StepLine {
            call: id,
            action: call.action.clone(),
            effect,
            end: StepEnd::Held(why),
            shown: StepShown::Full,
        });
    }

    /// The planner's last reply could not be read as a call: a line in the history says why.
    pub fn unread(&mut self, fault: docket_core::ReplyFault) {
        let id = CallId(self.next_call);
        self.next_call += 1;
        self.history.extend(StepLine::unread(id, fault));
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
                    // A placeholder until the router's own card (`Session.Handles`) replaces it.
                    self.handles.push(HandleCard {
                        handle,
                        shape: HandleShape::Text,
                        from,
                        size: CharCount(0),
                    });
                }
                Reveal::Handle(handle)
            }
            plain => Reveal::Plain(plain),
        }
    }
}
