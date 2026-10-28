//! The loop: what `next_event` does with each message the agent sends, and how a staged request
//! runs. See `backend` for the rules this keeps.

use super::backend::{AcpBackend, Called, Phase, Seams, Staged};
use super::handlers::ok;
use super::link::{FromAgent, Plain, Reply, Work};
use acp_core::call::AgentCall;
use acp_core::fault;
use acp_core::taint::TaintSource;
use agent_client_protocol::schema::v1::{
    CreateElicitationResponse, ElicitationAction, Error, RequestPermissionOutcome,
    RequestPermissionResponse, SessionNotification,
};
use docket_core::{FILES_READ, FILES_WRITE, StepLine, StepShown, TERMINAL_RUN, acp_agent_action};
use docket_session::{BackendEvent, CallEvent, CallOpen, TurnEnd};
use docket_vocab::CallId;
use prov::Effect;
use serde_json::Value;
use std::time::Duration;

/// How often a pending `terminal/wait_for_exit` looks at its command when the agent is silent.
const WAIT_POLL: Duration = Duration::from_millis(50);

/// What woke the loop.
enum Next {
    /// The agent sent something (boxed: a request is hundreds of bytes, a tick none).
    Said(Box<FromAgent>),
    /// A pending wait should look again.
    Tick,
    /// The agent closed the connection, and everything it sent has been taken.
    Gone,
}

impl<X: Seams> AcpBackend<X> {
    /// Answers `to`. An agent that can no longer be answered is dead.
    fn reply(&mut self, to: Reply, answer: Result<Value, Error>) {
        if to.respond_with_result(answer).is_err() {
            self.dead = true;
        }
    }

    /// The next message, or a tick when a pending wait should look again.
    async fn hear_next(&mut self) -> Next {
        let Some(live) = self.live.as_mut() else {
            return Next::Gone;
        };
        if self.waiters.is_empty() {
            return live
                .link
                .next()
                .await
                .map_or(Next::Gone, |said| Next::Said(Box::new(said)));
        }
        tokio::select! {
            said = live.link.next() => said.map_or(Next::Gone, |said| Next::Said(Box::new(said))),
            () = tokio::time::sleep(WAIT_POLL) => Next::Tick,
        }
    }

    fn poll_waiters(&mut self) {
        let waiting = std::mem::take(&mut self.waiters);
        for (reply, asked) in waiting {
            match self.performer.poll_wait(&asked) {
                Ok(None) => self.waiters.push((reply, asked)),
                Ok(Some(done)) => self.reply(reply, ok(&done)),
                Err(error) => self.reply(reply, Err(error)),
            }
        }
    }

    pub(super) async fn pull(&mut self) -> Option<BackendEvent> {
        loop {
            if let Some(event) = self.ready.pop_front() {
                return Some(event);
            }
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
            // An agent silent for the whole span is gone, as if it had closed its output.
            let heard = tokio::time::timeout(self.waits.person, self.hear_next())
                .await
                .unwrap_or(Next::Gone);
            self.adopt_trip();
            match heard {
                Next::Gone => self.dead = true,
                Next::Tick => self.poll_waiters(),
                Next::Said(said) => {
                    self.take(*said);
                    self.poll_waiters();
                }
            }
        }
    }

    /// The router paused the session on a call over the tool edge: the agent is told to stop,
    /// and the turn ends `Paused` when it does, as for a file call.
    fn adopt_trip(&mut self) {
        if self.pausing.is_some() {
            return;
        }
        let Some(live) = self.live.as_ref() else {
            return;
        };
        if let Some(trip) = live.edge.as_ref().and_then(|e| e.take_trip()) {
            self.pausing = Some(trip);
            self.tell_cancel();
        }
    }

    /// Tells the agent to cancel its session; nothing when it has none.
    fn tell_cancel(&mut self) {
        let Some(live) = self.live.as_ref() else {
            return;
        };
        let Some(agent) = live.agent.as_ref() else {
            return;
        };
        if live.link.cancel(agent).is_err() {
            self.dead = true;
        }
    }

    fn take(&mut self, said: FromAgent) {
        match said {
            FromAgent::Prompt(outcome) => self.turn_over(outcome),
            FromAgent::Update(note) => self.hear(*note),
            FromAgent::Ruled(work, reply) => self.stage(work, reply),
            FromAgent::Plain(plain, reply) => self.answer_plain(plain, reply),
        }
    }

    fn hear(&mut self, note: SessionNotification) {
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

    /// A request the router rules on: held for its turn, or turned away while the turn ends.
    fn stage(&mut self, work: Work, reply: Reply) {
        let refusing = self.cancelling || self.pausing.is_some();
        match work {
            Work::Permission(_) if refusing => {
                let cancelled = RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled);
                self.reply(reply, ok(&cancelled));
            }
            _ if refusing => self.reply(reply, Err(fault::not_now("the turn is ending"))),
            work => self.hold(work, reply),
        }
    }

    fn hold(&mut self, work: Work, reply: Reply) {
        let (action, effect) = match &work {
            Work::Read(_) => (FILES_READ, Effect::Read),
            Work::Write(_) => (FILES_WRITE, Effect::UndoableWrite),
            Work::Create(_) => (TERMINAL_RUN, docket_core::EXECUTE_AS),
            Work::Permission(_) => ("", Effect::Read),
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
            work,
            reply,
            announced: call.is_none(),
            call,
            stage: None,
            n: self.next_ask,
        });
    }

    /// A request answered at once: a terminal's output, kill and release never ask and never
    /// block; its wait is answered when the command has ended.
    fn answer_plain(&mut self, plain: Plain, reply: Reply) {
        let answer = match plain {
            Plain::Wait(asked) => {
                self.waiters.push((reply, asked));
                return;
            }
            Plain::Output(asked) => self.performer.terminal_output(&asked).and_then(|a| ok(&a)),
            Plain::Kill(asked) => self.performer.terminal_kill(&asked).and_then(|a| ok(&a)),
            Plain::Release(asked) => self.performer.terminal_release(&asked).and_then(|a| ok(&a)),
            Plain::Elicitation => ok(&CreateElicitationResponse::new(ElicitationAction::Decline)),
        };
        self.reply(reply, answer);
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
        let (work, n) = (staged.work.clone(), staged.n);
        let ran = self.run(&work, n).await;
        // From here to the end of the function nothing is awaited: the call has run, and its
        // reply, its end and the clearing of `staged` happen together.
        let staged = self.staged.take()?;
        self.reply(staged.reply, ran.reply);
        if let (Some(trip), None) = (ran.paused, self.pausing)
            && self.live.is_some()
        {
            // The router paused the session: the agent is told to stop, and the turn ends
            // `Paused` when it does.
            self.pausing = Some(trip);
            self.tell_cancel();
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

    /// Forgets the staged request, and the host's hold on it.
    pub(super) fn drop_staged(&mut self) {
        if let Some(stage) = self.staged.take().and_then(|s| s.stage) {
            self.performer.drop_stage(&stage);
        }
    }

    /// The host's hold on the staged request: made now if this is its first run, the same one
    /// if the run is being repeated.
    pub(super) fn stage_once(
        &mut self,
        make: impl FnOnce(&super::performer::Performer<X::Files, X::Sandbox>) -> acp_core::call::StageId,
    ) -> acp_core::call::StageId {
        if let Some(stage) = self.staged.as_ref().and_then(|s| s.stage.clone()) {
            return stage;
        }
        let stage = make(&self.performer);
        if let Some(staged) = self.staged.as_mut() {
            staged.stage = Some(stage.clone());
        }
        stage
    }

    /// Stops the turn in progress: a call that is staged and has not run never will, the agent
    /// is told "cancelled", and the turn ends when the agent answers its prompt.
    pub(super) fn stop_turn(&mut self) {
        if self.phase != Phase::Answering || self.cancelling {
            return;
        }
        self.cancelling = true;
        if let Some(staged) = self.staged.take() {
            // The router may still rule "yes" on the flight that outlives this: with the hold
            // gone it finds no request, and nothing runs after the agent heard "cancelled".
            if let Some(stage) = &staged.stage {
                self.performer.drop_stage(stage);
            }
            if let Some(call) = &staged.call {
                // The announcement may still be unread; an orphan `Started` must not survive.
                self.ready.retain(|e| {
                    !matches!(e, BackendEvent::Call(CallEvent::Started(o)) if o.call == call.call)
                });
            }
            match staged.work {
                Work::Permission(_) => {
                    let cancelled =
                        RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled);
                    self.reply(staged.reply, ok(&cancelled));
                }
                _ => self.reply(staged.reply, Err(fault::not_now("cancelled"))),
            }
        }
        self.tell_cancel();
    }
}
