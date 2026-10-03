//! Messages: the one model between the person, the companion, its workers, the computer-use runs
//! and the other Spaces. The router stamps, labels and delivers; companiond reads what waits for
//! the agents it runs, puts each message in the task it is for, and lets a request or a report
//! start the loop. A message is input and carries no authority: a request in it is planned and
//! gated by the receiving task like any call it thought of itself, and its label is already in
//! that task's taint (the router joined it at delivery).

use crate::fault::ServeFault;
use crate::runtime::Companiond;
use agent_loop::{AskedPerson, CompletionNote, LoopInput, LoopPhase, SideInput, attention_of};
use docket_client::{ClientError, Transport as IntentsTransport};
use docket_core::{
    AppRefusal, CallRefusal, CallRequest, Delivery, DenyCode, DraftPart, Follow, InboundLine,
    InboundPart, InboxAsk, LeadText, MessageDraft, Outcome, ParamName, Preview, Reveal,
    RosterDetail, RosterFull, RosterLine, RosterState, SendRefusal, SessionOpen, TargetValue,
    TaskStart, TurnId, TurnSource, TurnVia, Undoable, Value, WireRefusal,
};
use porter_client::Transport as InferTransport;
use porter_core::{AppName, Count};
use prov::{
    Address, AgentRef, Confidentiality, Integrity, Label, Labelled, MessageKind, ReportStatus,
    Source, SpaceId, TaskId,
};
use std::collections::BTreeSet;

fn status_state(status: ReportStatus) -> RosterState {
    match status {
        ReportStatus::Done => RosterState::Done,
        ReportStatus::Failed => RosterState::Failed,
        ReportStatus::Cancelled => RosterState::Cancelled,
        ReportStatus::Progress => RosterState::Working,
    }
}

fn first_text(line: &InboundLine) -> Option<&str> {
    line.parts.iter().find_map(|p| match p {
        InboundPart::Text(Reveal::Plain(t)) => Some(t.as_str()),
        _ => None,
    })
}

/// The number of a message id (`m-12`), for the turn id of a message the person sent.
fn turn_number(line: &InboundLine) -> u64 {
    line.id
        .as_str()
        .rsplit('-')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

/// What the typed parts of a report say: outcomes it names, and things it names.
fn counts(line: &InboundLine) -> (Count, Count) {
    let of = |f: fn(&InboundPart) -> bool| {
        Count(u32::try_from(line.parts.iter().filter(|p| f(p)).count()).unwrap_or(u32::MAX))
    };
    (
        of(|p| matches!(p, InboundPart::Outcome(_))),
        of(|p| matches!(p, InboundPart::Entity(_))),
    )
}

fn app_label(app: AppName) -> Label {
    Label {
        integrity: Integrity::Trusted,
        confidentiality: Confidentiality::Public,
        classes: BTreeSet::new(),
        sources: BTreeSet::from([Source::App(app)]),
    }
}

impl<P: InferTransport, I: IntentsTransport> Companiond<P, I> {
    /// `Intents1.Message.Arrived`: reads the inbox and feeds each message to the right task as
    /// input (a request is evaluated under that task's own policy; nothing in it widens it), then
    /// runs what the messages started.
    pub async fn arrived(&mut self) -> Result<(), ServeFault> {
        self.pull().await?;
        self.drain().await
    }

    /// Runs the loops that messages started, one after another.
    pub(crate) async fn drain(&mut self) -> Result<(), ServeFault> {
        while let Some((task, input)) = self.triggers.pop_front() {
            // A worker's loop is run from inside the loop of the task that spawned it: the one
            // place the machine calls itself, so its future is boxed here.
            Box::pin(self.run(&task, input)).await?;
        }
        Ok(())
    }

    /// Reads what waits for the companion and each of its workers, and puts each message in the
    /// task it is for. Nothing runs here.
    pub(crate) async fn pull(&mut self) -> Result<(), ServeFault> {
        let workers = self
            .runtimes
            .values()
            .filter(|rt| matches!(rt.agent, AgentRef::Worker { .. }))
            .map(|rt| rt.agent.clone());
        let agents: Vec<AgentRef> = std::iter::once(AgentRef::Companion)
            .chain(workers)
            .collect();
        for agent in agents {
            let lines = self
                .intents
                .inbox(InboxAsk {
                    agent: agent.clone(),
                    after: None,
                })
                .await
                .map_err(ServeFault::Router)?;
            for line in lines {
                self.deliver(&agent, line);
            }
        }
        Ok(())
    }

    /// The task a message for `agent` is for. A worker is its own task. The companion is one
    /// identity over many tasks: a message from its own Space is for a task there, one from
    /// another Space for a task elsewhere, the front task first.
    fn recipient(&self, agent: &AgentRef, line: &InboundLine) -> Option<TaskId> {
        if let AgentRef::Worker { task } = agent {
            return self.runtimes.contains_key(task).then(|| task.clone());
        }
        let live = |t: &TaskId| {
            self.tasks
                .get(t)
                .is_some_and(|s| !matches!(s.phase, LoopPhase::Finished(_)))
        };
        let fits = |t: &TaskId| {
            self.runtimes.get(t).is_some_and(|rt| {
                rt.agent == AgentRef::Companion
                    && (rt.space == line.from.space) == (line.crossing == prov::Crossing::Within)
            })
        };
        let front = self.front.iter().filter(|t| live(t) && fits(t)).cloned();
        let newest = self
            .runtimes
            .iter()
            .filter(|(t, _)| live(t) && fits(t))
            .max_by_key(|(_, rt)| rt.started)
            .map(|(t, _)| t.clone());
        front.chain(newest).next()
    }

    /// Puts one message in its task: the inbox the planner reads, the side conversation it may
    /// be part of, the roster line a report moves, and the loop input it starts.
    fn deliver(&mut self, agent: &AgentRef, line: InboundLine) {
        let Some(task) = self.recipient(agent, &line) else {
            self.unplaced.push_back(line);
            return;
        };
        let Some(rt) = self.runtimes.get_mut(&task) else {
            return;
        };
        rt.inbox.push(line.clone());
        let (space, is_worker) = (
            rt.space.clone(),
            matches!(rt.agent, AgentRef::Worker { .. }),
        );
        let to = rt.agent.clone();
        if line.from.agent == AgentRef::User && is_worker {
            self.told_by_message(&to, &space, &line);
        }
        match line.kind {
            MessageKind::Report { status } => self.reported(&task, &line, status),
            MessageKind::Request => self.trigger(&task, LoopInput::Messaged),
            MessageKind::Note => {}
        }
    }

    fn trigger(&mut self, task: &TaskId, input: LoopInput) {
        if !self.running.contains(task) {
            self.triggers.push_back((task.clone(), input));
        }
    }

    /// A subagent's report: its roster line moves, and the front task gets a typed note.
    fn reported(&mut self, task: &TaskId, line: &InboundLine, status: ReportStatus) {
        let from = line.from.agent.clone();
        let (steps, values) = counts(line);
        let ours = self.runtimes.values().any(|rt| rt.agent == from);
        if !ours {
            let told = self.told.get(&from).cloned();
            self.known.insert(
                from.clone(),
                RosterLine {
                    agent: from.clone(),
                    space: line.from.space.clone(),
                    state: status_state(status),
                    detail: RosterDetail::Full(Box::new(RosterFull {
                        goal: Reveal::Plain(String::new()),
                        last: None,
                        told,
                    })),
                },
            );
        }
        self.trigger(
            task,
            LoopInput::Completed(CompletionNote {
                agent: from,
                status,
                steps,
                values,
                attention: attention_of(status, AskedPerson::No),
            }),
        );
        self.publish();
    }

    /// The person's own turn to a subagent, as a message: their words are trusted, the roster
    /// line shows the first 80 characters at once, and the side conversation is tracked.
    fn told_by_message(&mut self, to: &AgentRef, space: &SpaceId, line: &InboundLine) {
        let Some(text) = first_text(line).map(str::to_owned) else {
            return;
        };
        let turn = docket_core::UserTurn {
            id: TurnId(turn_number(line)),
            text,
            at: self.clock.now(),
            from: TurnSource::Launcher,
            via: TurnVia::Typed,
        };
        self.told(to.clone(), space.clone(), turn);
    }

    /// The person said something to a subagent. Their words are recorded as theirs, the front
    /// agent's roster line shows them at once, and the conversation is tracked until it ends.
    pub fn told(&mut self, to: AgentRef, space: SpaceId, turn: docket_core::UserTurn) {
        self.told.insert(to.clone(), LeadText::of(&turn.text));
        let (side, _rederive) = agent_loop::side_step(
            std::mem::take(&mut self.side),
            SideInput::Told { to, space, turn },
            &self.config.idle,
        );
        self.side = side;
        self.publish();
    }

    /// A worker for a goal: its own session (never wider than its parent's policy), the goal
    /// delivered as a request from its parent, and its loop run to the end.
    pub(crate) async fn spawn_worker(
        &mut self,
        parent: &TaskId,
        start: TaskStart,
    ) -> Result<TaskId, ServeFault> {
        let (space, session) = {
            let rt = self
                .runtimes
                .get(parent)
                .ok_or(ServeFault::UnknownSession)?;
            (rt.space.clone(), rt.session.clone())
        };
        self.spawned += 1;
        let worker =
            TaskId::parse(&format!("w-{}", self.spawned)).map_err(|_| ServeFault::Malformed)?;
        let agent = AgentRef::Worker {
            task: worker.clone(),
        };
        let open = SessionOpen {
            space: space.clone(),
            agent: agent.clone(),
            parent: Some(parent.clone()),
        };
        let opened = self
            .intents
            .session_open(open.clone())
            .await
            .map_err(ServeFault::Router)?;
        self.adopt(&opened, &open);
        self.intents
            .send(
                session,
                MessageDraft {
                    to: Address::new(agent, space),
                    thread: None,
                    in_reply_to: None,
                    kind: MessageKind::Request,
                    parts: vec![DraftPart::Text(start.goal)],
                },
            )
            .await
            .map_err(ServeFault::Router)?;
        self.publish();
        self.pull().await?;
        self.drain().await?;
        Ok(worker)
    }

    /// The answer of `companion.task.start`: the new task's id, in the app's own trusted words.
    pub(crate) fn outcome_text(&self, worker: &TaskId) -> Outcome {
        let label = AppName::parse("org.quire.Companion")
            .map(app_label)
            .unwrap_or_else(|_| crate::args::planner_label());
        Outcome {
            value: Some(Labelled {
                value: Value::Text(worker.as_str().to_owned()),
                label,
            }),
            said: None,
            show: Preview::None,
            undo: Undoable::No,
            follow: Follow::Nothing,
        }
    }

    /// `companion.task.message`: its text goes, as a request, to the task its target names.
    pub(crate) async fn send_to_target(
        &mut self,
        from: &TaskId,
        call: &CallRequest,
    ) -> Result<Outcome, CallRefusal> {
        let TargetValue::Entities(targets) = &call.target else {
            return Err(bad("target"));
        };
        let entity = targets.first().ok_or_else(|| bad("target"))?;
        let to_task = TaskId::parse(entity.key.as_str()).map_err(|_| bad("target"))?;
        let (agent, space) = self
            .runtimes
            .get(&to_task)
            .map(|rt| (rt.agent.clone(), rt.space.clone()))
            .ok_or_else(|| CallRefusal::App(AppRefusal::NotFound(entity.clone())))?;
        let text = ParamName::parse("text").map_err(|_| bad("text"))?;
        let part = match call.args.get(&text).map(|a| &a.value) {
            Some(Value::Text(t)) => DraftPart::Text(prov::MessageText::new(t.clone())),
            Some(Value::Handle(h)) => DraftPart::Handle(*h),
            _ => return Err(bad("text")),
        };
        self.message(
            from,
            Address::new(agent, space),
            MessageKind::Request,
            vec![part],
        )
        .await
        .map(|_| self.outcome_nothing())
        .map_err(|error| match error {
            ServeFault::Router(ClientError::Refused(WireRefusal::Send(
                SendRefusal::NoRecipient,
            ))) => CallRefusal::App(AppRefusal::NotFound(entity.clone())),
            _ => CallRefusal::Denied(DenyCode::NotAllowed),
        })
    }

    fn outcome_nothing(&self) -> Outcome {
        Outcome {
            value: None,
            said: None,
            show: Preview::None,
            undo: Undoable::No,
            follow: Follow::Nothing,
        }
    }

    /// Sends a message from a task: to a worker, a run, or an agent in another Space. The router
    /// stamps the sender from the task's session and labels the message with what the task has
    /// read, so taint travels with it; it is delivered as input and grants the receiver nothing,
    /// and across Spaces it grants no memory read.
    pub async fn message(
        &mut self,
        from: &TaskId,
        to: Address,
        kind: MessageKind,
        parts: Vec<DraftPart>,
    ) -> Result<Delivery, ServeFault> {
        let session = self
            .runtimes
            .get(from)
            .map(|rt| rt.session.clone())
            .ok_or(ServeFault::UnknownSession)?;
        self.intents
            .send(
                session,
                MessageDraft {
                    to,
                    thread: None,
                    in_reply_to: None,
                    kind,
                    parts,
                },
            )
            .await
            .map_err(ServeFault::Router)
    }
}

fn bad(name: &str) -> CallRefusal {
    ParamName::parse(name).map_or(CallRefusal::Timeout, |param| CallRefusal::BadArgs {
        param,
        why: docket_core::ArgFault::WrongType,
    })
}
