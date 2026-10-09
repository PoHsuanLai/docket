//! Carrying out the loop. `agent_step` says what happens; this turns each effect into a call to
//! the router, the reader or the planner and feeds the end back in as the next input, until the
//! task is waiting for the person or over. Nothing here decides what is allowed: every call goes
//! to the router, which gates it, and a refusal comes back as a code.

use crate::fault::ServeFault;
use crate::plan::phase_of;
use crate::read::{COMPANION_APP, answer_read};
use crate::runtime::Companion;
use crate::seams::{Now, Surface};
use crate::tap::{Go, NoTap, Tap};
use crate::task::Failure;
use agent_loop::{
    IdleInput, LoopEffect, LoopInput, LoopPhase, LoopState, ModelOutput, agent_step, assemble,
    idle_step,
};
use companion_wire::{AnswerPhase, RefusalWire, declined_text};
use docket_client::{ClientError, PerformEvent, Transport as IntentsTransport};
use docket_core::{
    CallId, CallProgress, CallRefusal, CallRequest, ReaderAsk, StepEnd, WireRefusal,
};
use docket_planner::PlanFault;
use docket_session::CallOpen;
use porter_client::Transport as InferTransport;
use prov::{Effect, TaskId};
use std::collections::VecDeque;

/// The task-starting action of the built-in provider; companiond carries it out itself: it runs
/// the loops of the tasks it starts.
const TASK_START: &str = "companion.task.start";

/// What a call to the router came to, as the loop is told.
pub fn refusal_of(error: ClientError) -> CallRefusal {
    match error {
        ClientError::Refused(WireRefusal::Call(refusal)) => refusal,
        ClientError::Refused(_) | ClientError::Transport(_) | ClientError::Unexpected => {
            CallRefusal::Timeout
        }
    }
}

/// What the person is told when the planner gave nothing usable and there is more to say than
/// "it failed".
fn refusal_text(fault: &PlanFault) -> Option<String> {
    match fault {
        PlanFault::Declined(declined) => Some(declined_text(declined)),
        PlanFault::CallInText => Some(
            "The model wrote a step as text instead of making it, so nothing was done.".to_owned(),
        ),
        PlanFault::Unavailable | PlanFault::Unreadable => None,
    }
}

fn idle_loop() -> LoopState {
    LoopState {
        phase: LoopPhase::Idle,
        turn: None,
        steps: 0,
        pending: vec![],
        guard: Default::default(),
    }
}

impl<P: InferTransport, I: IntentsTransport, K: Now, S: Surface> Companion<P, I, K, S> {
    /// Runs `first`, and what follows from it, to the end of what the task can do alone.
    pub(crate) async fn run(&mut self, task: &TaskId, first: LoopInput) -> Result<(), ServeFault> {
        self.run_tapped(task, first, &mut NoTap).await
    }

    /// [`Self::run`] with `tap` told of each step and holding each call at its gate.
    pub(crate) async fn run_tapped<T: Tap>(
        &mut self,
        task: &TaskId,
        first: LoopInput,
        tap: &mut T,
    ) -> Result<(), ServeFault> {
        if !self.running.insert(task.clone()) {
            return Ok(());
        }
        let result = self.run_inputs(task, first, tap).await;
        self.running.remove(task);
        self.publish();
        result
    }

    async fn run_inputs<T: Tap>(
        &mut self,
        task: &TaskId,
        first: LoopInput,
        tap: &mut T,
    ) -> Result<(), ServeFault> {
        let mut queue = VecDeque::from([first]);
        while let Some(input) = queue.pop_front() {
            let input = if self.shared.take_cancel(task) {
                queue.clear();
                LoopInput::Cancelled
            } else {
                input
            };
            let state = self.tasks.get(task).cloned().unwrap_or_else(idle_loop);
            let (next, effects) = agent_step(state, input);
            self.tasks.insert(task.clone(), next);
            let mut position = 0u64;
            for effect in effects {
                let more = self.carry_out(task, effect, &mut position, tap).await?;
                queue.extend(more);
            }
        }
        Ok(())
    }

    /// An answer reached a phase: the record of what the turn left, for a restart.
    async fn replied(&self, task: &TaskId) {
        // A finished task has its own record: nothing more is said of it.
        if matches!(
            self.tasks.get(task).map(|s| s.phase),
            Some(LoopPhase::Finished(_))
        ) {
            return;
        }
        let (Some(rt), Some(digest)) = (self.runtimes.get(task), self.digest_of(task)) else {
            return;
        };
        let record = companion_wire::SessionRecord::Replied {
            task: task.clone(),
            phase: rt.phase.clone(),
            digest,
        };
        self.record(&rt.session, &record).await;
    }

    /// Applies one input to the idle pass and returns what it asks for.
    pub(crate) fn idle_apply(&mut self, input: IdleInput) -> Vec<agent_loop::IdleEffect> {
        let (next, effects) = idle_step(self.idle.clone(), input, &self.config.idle);
        self.idle = next;
        effects
    }

    /// A turn of the person's: the idle pass yields, the loop runs, and what landed meanwhile is
    /// read.
    pub(crate) async fn run_interactive(
        &mut self,
        task: &TaskId,
        first: LoopInput,
    ) -> Result<(), ServeFault> {
        self.run_interactive_tapped(task, first, &mut NoTap).await
    }

    /// [`Self::run_interactive`] with `tap` told of each step.
    pub(crate) async fn run_interactive_tapped<T: Tap>(
        &mut self,
        task: &TaskId,
        first: LoopInput,
        tap: &mut T,
    ) -> Result<(), ServeFault> {
        self.shared.interrupt();
        self.idle_apply(IdleInput::InteractiveStarted);
        let ran = self.run_tapped(task, first, tap).await;
        self.replied(task).await;
        let ended = self.clock.now();
        self.idle_apply(IdleInput::InteractiveEnded(ended));
        ran?;
        self.arrived().await
    }

    async fn carry_out<T: Tap>(
        &mut self,
        task: &TaskId,
        effect: LoopEffect,
        position: &mut u64,
        tap: &mut T,
    ) -> Result<Vec<LoopInput>, ServeFault> {
        match effect {
            LoopEffect::AskPlanner => self.plan_turn(task, tap).await,
            LoopEffect::Call(call) => {
                let id = CallId(*position);
                *position += 1;
                let open = self.open_of(task, &call);
                if tap.gate(open).await == Go::Stop {
                    return Ok(vec![LoopInput::Cancelled]);
                }
                let inputs = self.perform(task, *call, id, tap).await?;
                if let Some(line) = self.runtimes.get(task).and_then(|rt| rt.history.last()) {
                    tap.ended(line);
                }
                Ok(inputs)
            }
            LoopEffect::Read(ask) => self.read(task, *ask).await,
            LoopEffect::Publish(phase) => {
                if let AnswerPhase::NeedsYou(need) = &phase {
                    tap.needs(need);
                }
                if let Some(rt) = self.runtimes.get_mut(task) {
                    rt.phase = phase;
                }
                self.publish_answer(task);
                Ok(vec![])
            }
            // The report that made the note is in the task's inbox, where the planner reads it as
            // a typed line; a refusal reached the planner as the coarse code in the history.
            LoopEffect::Held(call, why) => {
                self.hold(task, &call, why);
                Ok(vec![])
            }
            LoopEffect::Unread(fault) => {
                self.unread(task, fault);
                Ok(vec![])
            }
            LoopEffect::Refused(refusal) => {
                if let (CallRefusal::OverBudget(_), Some(rt)) =
                    (&refusal, self.runtimes.get_mut(task))
                {
                    rt.failure = Some(Failure::Refused(refusal));
                }
                Ok(vec![])
            }
            LoopEffect::Note(_) => Ok(vec![]),
            LoopEffect::CloseTask => {
                self.finish(task).await?;
                Ok(vec![])
            }
        }
    }

    /// One planner turn: assemble the view, ask the model, and hand the loop what it said.
    async fn plan_turn<T: Tap>(
        &mut self,
        task: &TaskId,
        tap: &mut T,
    ) -> Result<Vec<LoopInput>, ServeFault> {
        let spent = self.tasks.get(task).map_or(0, |s| s.steps);
        if spent >= self.config.budget.calls.0 {
            if let Some(rt) = self.runtimes.get_mut(task) {
                rt.failure = Some(Failure::Budget);
            }
            return Ok(vec![LoopInput::ModelFailed]);
        }
        let sources = self.sources(task).await;
        let view = assemble(&self.config.assembler, &sources);
        let reply = match self.planner.converse(&view).await {
            Ok(reply) => reply,
            Err(fault) => {
                if let Some(rt) = self.runtimes.get_mut(task) {
                    if let Some(text) = refusal_text(&fault) {
                        rt.refused = Some(RefusalWire::Failed(text));
                    }
                    rt.failure = Some(Failure::Model(fault));
                }
                return Ok(vec![LoopInput::ModelFailed]);
            }
        };
        if let Some(rt) = self.runtimes.get_mut(task) {
            rt.served = reply.served.clone();
            if !reply.route.is_empty() {
                rt.route = reply.route.clone();
            }
            if let Some(words) = &reply.said {
                rt.said.push(words.clone());
                tap.said(words);
            }
        }
        let said = reply.said.map(|w| LoopInput::Planned(ModelOutput::Say(w)));
        Ok(said
            .into_iter()
            .chain([LoopInput::Planned(reply.then)])
            .collect())
    }

    /// One call, through the router. The action that starts a task is carried out here as well
    /// (the router makes the task, the loop of the worker runs here); `companion.task.message` is
    /// the router's alone, like any other call.
    async fn perform<T: Tap>(
        &mut self,
        task: &TaskId,
        call: CallRequest,
        id: CallId,
        tap: &mut T,
    ) -> Result<Vec<LoopInput>, ServeFault> {
        if call.action.app.as_str() == COMPANION_APP && call.action.name.as_str() == TASK_START {
            return self.start_task(task, call, id).await;
        }
        let Some(rt) = self.runtimes.get(task) else {
            return Err(ServeFault::UnknownSession);
        };
        let (session, window) = (rt.session.clone(), rt.window.clone());
        let effect = self
            .planner
            .catalogue()
            .of(&call.action)
            .map_or(Effect::Read, |t| t.decl.effect);
        let label = self
            .planner
            .catalogue()
            .of(&call.action)
            .map(|t| t.decl.label.clone());
        self.plan_begin(task, &call, label, effect, id);
        let result = self
            .perform_watched(task, &call, id, session.clone(), window, tap)
            .await;
        if result.is_ok() {
            self.skill_loaded(task, &session, &call).await;
        }
        Ok(vec![self.ended(task, &call, effect, id, result)])
    }

    /// The call's own end, with the answer following the router's progress: the plan card shows
    /// the step, and `NeedsYou(Confirm)` stands while the sheet is up.
    async fn perform_watched<T: Tap>(
        &mut self,
        task: &TaskId,
        call: &CallRequest,
        id: CallId,
        session: prov::SessionId,
        window: Option<docket_core::WindowKey>,
        tap: &mut T,
    ) -> Result<docket_core::Outcome, CallRefusal> {
        let mut watch = match self
            .intents
            .perform_watched(call.clone(), Some(session), window)
            .await
        {
            Ok(watch) => watch,
            Err(error) => return Err(refusal_of(error)),
        };
        loop {
            match watch.next().await {
                Ok(PerformEvent::Progress(progress)) => {
                    self.plan_progress(task, id, &progress);
                    if let Some(AnswerPhase::NeedsYou(need)) =
                        self.runtimes.get(task).map(|rt| &rt.phase)
                    {
                        tap.needs(need);
                    }
                }
                Ok(PerformEvent::Done(end)) => return *end,
                Err(error) => return Err(refusal_of(error)),
            }
        }
    }

    /// The call as the one who reads the turn is told of it before it is made: the number its
    /// step line will carry, its action and its effect.
    fn open_of(&self, task: &TaskId, call: &CallRequest) -> CallOpen {
        let effect = self
            .planner
            .catalogue()
            .of(&call.action)
            .map_or(Effect::Read, |t| t.decl.effect);
        CallOpen {
            call: CallId(self.runtimes.get(task).map_or(0, |rt| rt.next_call)),
            action: call.action.clone(),
            effect,
        }
    }

    fn plan_begin(
        &mut self,
        task: &TaskId,
        call: &CallRequest,
        label: Option<docket_core::LabelText>,
        effect: Effect,
        id: CallId,
    ) {
        if let Some(rt) = self.runtimes.get_mut(task) {
            let label = label
                .or_else(|| docket_core::LabelText::parse(call.action.name.as_str()).ok())
                .or_else(|| docket_core::LabelText::parse("Step").ok());
            if let Some(label) = label {
                rt.plan.begin(id, call.action.clone(), label, effect);
            }
            rt.phase = AnswerPhase::Streaming;
        }
        self.publish_answer(task);
    }

    fn plan_progress(&mut self, task: &TaskId, id: CallId, progress: &CallProgress) {
        if let Some(rt) = self.runtimes.get_mut(task) {
            rt.plan.progress(id, progress);
            rt.phase = phase_of(progress);
        }
        self.publish_answer(task);
    }

    /// Records a call's end in the task and says it to the loop.
    pub(crate) fn ended(
        &mut self,
        task: &TaskId,
        call: &CallRequest,
        effect: Effect,
        id: CallId,
        result: Result<docket_core::Outcome, CallRefusal>,
    ) -> LoopInput {
        let end = match self.runtimes.get_mut(task) {
            Some(rt) => {
                let end = rt.record_call(call, effect, result);
                rt.plan.end(id, &end);
                if matches!(
                    rt.phase,
                    AnswerPhase::NeedsYou(companion_wire::NeedsYou::Confirm(_))
                ) {
                    rt.phase = AnswerPhase::Streaming;
                }
                end
            }
            None => StepEnd::Refused(CallRefusal::Timeout),
        };
        self.publish_answer(task);
        LoopInput::CallEnded(id, end)
    }

    /// `companion.task.start`: a call like any other (gated, budgeted, audited): the router opens
    /// the worker's session and answers the task and the session, which are adopted here, and the
    /// worker runs to its end before the spawning task goes on. The planner is told the task id.
    async fn start_task(
        &mut self,
        task: &TaskId,
        call: CallRequest,
        id: CallId,
    ) -> Result<Vec<LoopInput>, ServeFault> {
        let Some((session, window)) = self
            .runtimes
            .get(task)
            .map(|rt| (rt.session.clone(), rt.window.clone()))
        else {
            return Err(ServeFault::UnknownSession);
        };
        let performed = self
            .intents
            .perform(call.clone(), Some(session), window)
            .await
            .unwrap_or_else(|error| Err(refusal_of(error)));
        let result = match performed {
            Err(refusal) => Err(refusal),
            Ok(started) => match self.adopt_worker(task, &started).await {
                Ok(worker) => Ok(self.outcome_text(&worker, started)),
                Err(error) => Err(match error {
                    ServeFault::Router(error) => refusal_of(error),
                    _ => CallRefusal::Timeout,
                }),
            },
        };
        Ok(vec![self.ended(task, &call, Effect::Read, id, result)])
    }

    /// One read of the quarantined reader (`answer_read`), for the task.
    async fn read(&mut self, task: &TaskId, ask: ReaderAsk) -> Result<Vec<LoopInput>, ServeFault> {
        let Some(rt) = self.runtimes.get_mut(task) else {
            return Err(ServeFault::UnknownSession);
        };
        Ok(answer_read(&self.intents, rt, ask).await)
    }
}
