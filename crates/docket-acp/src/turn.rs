//! One prompt turn as a pure step machine: backend events and the editor's answers in, session
//! updates and orders out. The serve loop only carries them; nothing here reads a wire or a
//! clock. A call the editor stops (a reject, a read-only mode, a cancel) is never let go on: the
//! backend is cancelled and the turn ends `cancelled`.

use crate::calls;
use crate::covered::Covered;
use crate::mode::{Mode, Say};
use crate::permission::Verdict;
use agent_client_protocol_schema::v1::{SessionUpdate, StopReason};
use companion_wire::NeedsYou;
use docket_core::{AlwaysOffer, ConfirmRequest, Reveal};
use docket_session::SheetChoice;
use docket_session::{BackendEvent, CallEvent, CallOpen, TurnEnd};

/// Why the turn is being stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Why {
    Rejected,
    ReadOnly,
    Cancelled,
}

impl Why {
    fn sentence(self) -> &'static str {
        match self {
            Why::Rejected => "Declined in the editor.",
            Why::ReadOnly => "Not allowed in read-only mode.",
            Why::Cancelled => "Cancelled.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Running,
    Asking(CallOpen),
    Sheeting(Box<ConfirmRequest>),
    Stopping { call: Option<CallOpen>, why: Why },
    Done(Finish),
}

/// How the prompt request is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finish {
    /// With this stop reason.
    Stop(StopReason),
    /// With an error: the turn failed.
    Failed,
}

/// What the loop does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wants {
    /// The next backend event.
    Event,
    /// The editor's answer about this call.
    Answer(CallOpen),
    /// The editor's choice on the router's sheet.
    Sheet(Box<ConfirmRequest>),
    /// Nothing: answer the prompt.
    Over(Finish),
}

/// What the loop must carry out.
#[derive(Debug, Clone, PartialEq)]
pub enum Order {
    /// Send this `session/update`.
    Update(Box<SessionUpdate>),
    /// Ask the host to cancel the turn.
    CancelBackend,
}

/// The machine.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    mode: Mode,
    covered: Covered,
    state: State,
}

const WAITS: &str = "Waiting for you on the desktop.";
const PAUSED: &str = "The assistant paused and waits for you; send another message to go on.";
const HIDDEN: &str = "[something the assistant holds]";

fn words(of: Reveal<String>) -> String {
    match of {
        Reveal::Plain(text) => text,
        // A handle is never sent: the editor cannot replay it, and display text needs the
        // desktop's `Session.Display`.
        Reveal::Handle(_) => HIDDEN.to_owned(),
    }
}

fn needs(of: &NeedsYou) -> String {
    match of {
        NeedsYou::Confirm(_) | NeedsYou::Form(_) => WAITS.to_owned(),
        NeedsYou::Question { text, choices } if choices.is_empty() => text.clone(),
        NeedsYou::Question { text, choices } => format!("{text} ({})", choices.join(" / ")),
    }
}

impl Turn {
    /// A turn in `mode`, for a person who already said "always" to `covered`.
    pub fn new(mode: Mode, covered: Covered) -> Self {
        Self {
            mode,
            covered,
            state: State::Running,
        }
    }

    /// What the loop does next.
    pub fn wants(&self) -> Wants {
        match &self.state {
            State::Running | State::Stopping { .. } => Wants::Event,
            State::Asking(call) => Wants::Answer(call.clone()),
            State::Sheeting(sheet) => Wants::Sheet(sheet.clone()),
            State::Done(finish) => Wants::Over(*finish),
        }
    }

    fn stop(&mut self, call: Option<CallOpen>, why: Why) -> Vec<Order> {
        self.state = State::Stopping { call, why };
        vec![Order::CancelBackend]
    }

    /// The editor cancelled (or went away).
    pub fn cancelled(&mut self) -> Vec<Order> {
        match std::mem::replace(&mut self.state, State::Running) {
            State::Running => self.stop(None, Why::Cancelled),
            State::Asking(call) => self.stop(Some(call), Why::Cancelled),
            State::Sheeting(_) => self.stop(None, Why::Cancelled),
            other => {
                self.state = other;
                Vec::new()
            }
        }
    }

    /// The editor's person answered the permission request.
    pub fn answered(&mut self, verdict: Verdict) -> Vec<Order> {
        let State::Asking(call) = std::mem::replace(&mut self.state, State::Running) else {
            return Vec::new();
        };
        match verdict {
            Verdict::Allow => vec![Order::Update(Box::new(calls::running(&call)))],
            Verdict::Reject => self.stop(Some(call), Why::Rejected),
        }
    }

    /// The sheet was answered (the host has the choice): the turn goes on, and the router ends
    /// the call as the choice says.
    pub fn sheet_answered(&mut self, choice: SheetChoice) {
        let State::Sheeting(sheet) = std::mem::replace(&mut self.state, State::Running) else {
            return;
        };
        if let (SheetChoice::Always, AlwaysOffer::Offered(scope)) = (choice, &sheet.always) {
            let covered = std::mem::take(&mut self.covered);
            self.covered = covered.with(scope.action().clone());
        }
    }

    /// What the person has said "always" to, this turn included.
    pub fn covered(&self) -> Covered {
        self.covered.clone()
    }

    /// The backend's next event.
    pub fn event(&mut self, event: BackendEvent) -> Vec<Order> {
        if let State::Stopping { call, why } = &self.state {
            return match event {
                BackendEvent::TurnEnd(_) => {
                    let orders: Vec<Order> = call
                        .iter()
                        .map(|c| Order::Update(Box::new(calls::stopped(c, why.sentence()))))
                        .collect();
                    self.state = State::Done(Finish::Stop(StopReason::Cancelled));
                    orders
                }
                _ => Vec::new(),
            };
        }
        match event {
            BackendEvent::Words(text) => vec![Order::Update(Box::new(calls::say(words(text))))],
            BackendEvent::Thought(_) | BackendEvent::Usage(_) => Vec::new(),
            BackendEvent::Sheet(sheet) => {
                self.state = State::Sheeting(sheet);
                Vec::new()
            }
            BackendEvent::NeedsYou(need) => vec![Order::Update(Box::new(calls::say(needs(&need))))],
            BackendEvent::Call(CallEvent::Ended(step)) => {
                vec![Order::Update(Box::new(calls::ended(&step)))]
            }
            BackendEvent::Call(CallEvent::Started(open)) => self.started(open),
            BackendEvent::TurnEnd(end) => self.ended(end),
        }
    }

    fn started(&mut self, open: CallOpen) -> Vec<Order> {
        let first = Order::Update(Box::new(calls::started(&open)));
        let says = match self.mode.says(&open) {
            Say::Ask if self.covered.holds(&open.action) => Say::Proceed,
            other => other,
        };
        match says {
            Say::Proceed => vec![first, Order::Update(Box::new(calls::running(&open)))],
            Say::Ask => {
                self.state = State::Asking(open);
                vec![first]
            }
            Say::Stop => {
                let mut orders = vec![first];
                orders.extend(self.stop(Some(open), Why::ReadOnly));
                orders
            }
        }
    }

    fn ended(&mut self, end: TurnEnd) -> Vec<Order> {
        let (finish, said) = match end {
            TurnEnd::Done => (Finish::Stop(StopReason::EndTurn), None),
            TurnEnd::Failed => (Finish::Failed, None),
            TurnEnd::Refused => (Finish::Stop(StopReason::Refusal), None),
            TurnEnd::Cancelled => (Finish::Stop(StopReason::Cancelled), None),
            TurnEnd::Paused(_) => (Finish::Stop(StopReason::Refusal), Some(PAUSED)),
        };
        self.state = State::Done(finish);
        said.map(|s| Order::Update(Box::new(calls::say(s))))
            .into_iter()
            .collect()
    }
}
