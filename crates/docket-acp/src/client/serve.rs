//! The loop and the handlers: what `next_event` does with each line the agent sends, and how a
//! staged request runs. See `backend` for the rules this keeps.

use super::backend::{AcpBackend, Called, Phase, Seams, Staged};
use super::call::AgentCall;
use super::handlers::ok;
use super::intake::{Intake, Work, intake};
use super::rpc;
use super::taint::TaintSource;
use crate::fault;
use crate::out;
use crate::wire::Wire;
use agent_client_protocol_schema::rpc::RequestId;
use agent_client_protocol_schema::v1::{
    CreateElicitationResponse, ElicitationAction, Error, RequestPermissionOutcome,
    RequestPermissionResponse,
};
use docket_core::{
    CallId, FILES_READ, FILES_WRITE, StepLine, StepShown, TERMINAL_RUN, acp_agent_action,
};
use docket_session::{BackendEvent, CallEvent, CallOpen, TurnEnd};
use prov::Effect;
use serde_json::Value;
use std::time::Duration;

/// How often a pending `terminal/wait_for_exit` looks at its command when the agent is silent.
const WAIT_POLL: Duration = Duration::from_millis(50);

/// An error reply line.
pub(super) fn refusal(id: &RequestId, message: &str) -> String {
    out::failure(id, &fault::not_now(message))
}

impl<X: Seams> AcpBackend<X> {
    fn reply(&mut self, id: &RequestId, answer: Result<Value, Error>) {
        let line = match answer {
            Ok(value) => out::reply(id, &value),
            Err(error) => out::failure(id, &error),
        };
        self.outbox.push_back(line);
    }

    async fn flush(&mut self) {
        while let Some(line) = self.outbox.front().cloned() {
            let Some(live) = self.live.as_mut() else {
                self.outbox.clear();
                return;
            };
            if live.wire.write_line(line).await.is_err() {
                self.dead = true;
                self.outbox.clear();
                return;
            }
            self.outbox.pop_front();
        }
    }

    /// The next line, or a tick (an empty string) when a pending wait should look again.
    async fn line(&mut self) -> Option<String> {
        let live = self.live.as_mut()?;
        if self.waiters.is_empty() {
            return live.wire.read_line().await;
        }
        tokio::select! {
            line = live.wire.read_line() => line,
            () = tokio::time::sleep(WAIT_POLL) => Some(String::new()),
        }
    }

    fn poll_waiters(&mut self) {
        let waiting = std::mem::take(&mut self.waiters);
        for (id, params) in waiting {
            match self.performer.poll_wait(params.clone()) {
                Ok(None) => self.waiters.push((id, params)),
                Ok(Some(value)) => self.outbox.push_back(out::reply(&id, &value)),
                Err(error) => self.outbox.push_back(out::failure(&id, &error)),
            }
        }
    }

    pub(super) async fn pull(&mut self) -> Option<BackendEvent> {
        loop {
            if let Some(event) = self.ready.pop_front() {
                return Some(event);
            }
            self.flush().await;
            self.tell_owed().await;
            if let Some(event) = self.step_staged().await {
                return Some(event);
            }
            if self.staged.is_some() {
                continue;
            }
            if self.dead && self.phase == Phase::Answering {
                self.finish_turn(TurnEnd::Failed);
                continue;
            }
            if self.phase != Phase::Answering {
                return None;
            }
            // Whatever we owe the agent goes out before we wait to hear from it.
            self.flush().await;
            match self.line().await {
                None => self.dead = true,
                Some(line) if line.is_empty() => self.poll_waiters(),
                Some(line) => {
                    self.take(intake(&line));
                    self.poll_waiters();
                }
            }
        }
    }

    fn take(&mut self, heard: Intake) {
        match heard {
            Intake::Reply { id, outcome } if self.prompt.as_ref() == Some(&id) => {
                self.turn_over(outcome);
            }
            Intake::Update(note) => self.hear(*note),
            Intake::Request { id, work } => self.stage(id, work),
            Intake::Reply { .. } | Intake::Ignore => {}
        }
    }

    fn hear(&mut self, note: agent_client_protocol_schema::v1::SessionNotification) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        if live.agent.as_ref() != Some(&note.session_id) {
            return;
        }
        let heard = live.reported.hear(&mut self.next_call, note.update);
        if let Some(kind) = heard.taint {
            live.tainted_by.get_or_insert(TaintSource::Reported(kind));
            // What a tool the agent ran itself brought in is never seen here.
            self.performer.note_unseen(&live.session);
            self.next_ask += 1;
            self.owed.push_back((self.next_ask, kind));
        }
        self.ready.extend(heard.events);
    }

    /// Tells the router what the agent's own tools brought in, so the session is tainted by it.
    /// What the router says changes nothing: it already happened.
    async fn tell_owed(&mut self) {
        while let Some(&(n, kind)) = self.owed.front() {
            let _ = self.ask(n, &AgentCall::Reported(kind)).await;
            self.owed.pop_front();
        }
    }

    fn stage(&mut self, id: RequestId, work: Work) {
        let refusing = self.cancelling || self.pausing.is_some();
        match work {
            Work::Unknown => self.reply(&id, Err(Error::method_not_found())),
            Work::Bad => self.reply(
                &id,
                Err(fault::invalid("the parameters do not fit the method")),
            ),
            Work::Elicitation => {
                let answer = CreateElicitationResponse::new(ElicitationAction::Decline);
                self.reply(&id, ok(&answer));
            }
            Work::Permission(_) if refusing => {
                self.reply(
                    &id,
                    ok(&RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    )),
                );
            }
            Work::Terminal { method, params } => self.terminal_other(&id, &method, params),
            _ if refusing => self.reply(&id, Err(fault::not_now("the turn is ending"))),
            work => self.hold(id, work),
        }
    }

    fn hold(&mut self, id: RequestId, work: Work) {
        let (action, effect) = match &work {
            Work::Read(_) => (FILES_READ, Effect::Read),
            Work::Write(_) => (FILES_WRITE, Effect::UndoableWrite),
            Work::Create { .. } => (TERMINAL_RUN, docket_core::EXECUTE_AS),
            _ => ("", Effect::Read),
        };
        let call = acp_agent_action(action)
            .filter(|_| !action.is_empty())
            .map(|action| {
                self.next_call += 1;
                Called {
                    call: CallId(self.next_call),
                    action,
                    effect,
                }
            });
        self.next_ask += 1;
        self.staged = Some(Staged {
            id,
            work,
            announced: call.is_none(),
            call,
            n: self.next_ask,
        });
    }

    fn terminal_other(&mut self, id: &RequestId, method: &str, params: Value) {
        if method == agent_client_protocol_schema::v1::CLIENT_METHOD_NAMES.terminal_wait_for_exit {
            self.waiters.push((id.clone(), params));
            return;
        }
        // Kill, release and output never ask and never block.
        let answer = self
            .performer
            .terminal_other(method, params)
            .unwrap_or_else(|| Err(fault::internal("could not answer")));
        self.reply(id, answer);
    }

    /// Announces the staged call, or runs it. Returns the announcement when there is one.
    async fn step_staged(&mut self) -> Option<BackendEvent> {
        let staged = self.staged.as_mut()?;
        if !staged.announced {
            staged.announced = true;
            let call = staged.call.clone()?;
            return Some(BackendEvent::Call(CallEvent::Started(CallOpen {
                call: call.call,
                action: call.action,
                effect: call.effect,
            })));
        }
        let staged = self.staged.clone()?;
        let ran = self.run(&staged).await;
        // From here to the end of the function nothing is awaited: the call has run, and its
        // reply, its end and the clearing of `staged` happen together.
        self.staged = None;
        self.reply(&staged.id, ran.reply);
        if let (Some(trip), None) = (ran.paused, self.pausing)
            && let Some(live) = self.live.as_ref()
        {
            // The router paused the session: the agent is told to stop, and the turn ends
            // `Paused` when it does.
            self.pausing = Some(trip);
            self.outbox
                .push_back(live.agent.as_ref().map(rpc::cancel).unwrap_or_default());
        }
        staged.call.map(|call| {
            BackendEvent::Call(CallEvent::Ended(StepLine {
                call: call.call,
                action: call.action,
                effect: call.effect,
                end: ran.end,
                shown: StepShown::Full,
                with: Vec::new(),
            }))
        })
    }

    /// Stops the turn in progress: a call that is staged and has not run never will, the agent
    /// is told "cancelled", and the turn ends when the agent answers its prompt.
    pub(super) async fn stop_turn(&mut self) {
        if self.phase != Phase::Answering || self.cancelling {
            return;
        }
        self.cancelling = true;
        if let Some(staged) = self.staged.take() {
            if let Some(call) = &staged.call {
                // The announcement may still be unread; an orphan `Started` must not survive.
                self.ready.retain(|e| {
                    !matches!(e, BackendEvent::Call(CallEvent::Started(o)) if o.call == call.call)
                });
            }
            match staged.work {
                Work::Permission(_) => {
                    let answer =
                        RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled);
                    self.reply(&staged.id, ok(&answer));
                }
                _ => self.reply(&staged.id, Err(fault::not_now("cancelled"))),
            }
        }
        if let Some(agent) = self.live.as_ref().and_then(|l| l.agent.clone()) {
            self.outbox.push_back(rpc::cancel(&agent));
        }
        self.flush().await;
    }
}
