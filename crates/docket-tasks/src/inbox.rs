//! Messages: the one model between the person, the companion, its workers, the computer-use runs
//! and the other Spaces. The router stamps, labels and delivers; companiond reads what waits for
//! the agents it runs, puts each message in the task it is for, and lets a request or a report
//! start the loop. A message is input and carries no authority: a request in it is planned and
//! gated by the receiving task like any call it thought of itself, and its label is already in
//! that task's taint (the router joined it at delivery).

use crate::fault::ServeFault;
use crate::runtime::Companion;
use crate::seams::{Now, Surface};
use agent_loop::{AskedPerson, CompletionNote, LoopInput, LoopPhase, SideInput, attention_of};
use docket_client::Transport as IntentsTransport;
use docket_core::{
    Delivery, DraftPart, InboundLine, InboundPart, InboxAsk, LeadText, MessageDraft, Outcome,
    Reveal, RosterDetail, RosterFull, RosterLine, RosterState, SessionOpen, SessionOpened, TurnId,
    TurnSource, TurnVia, Value,
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

impl<P: InferTransport, I: IntentsTransport, K: Now, S: Surface> Companion<P, I, K, S> {
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
        // The line says which Space it is for: the task it lands in is one of the companion's own
        // there (a message from another Space is for a task of the Space it was sent to).
        let fits = |t: &TaskId| {
            self.runtimes
                .get(t)
                .is_some_and(|rt| rt.agent == AgentRef::Companion && rt.space == line.to.space)
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

    /// Takes up the worker the router started for a call of `companion.task.start`: its own
    /// session (never wider than its parent's policy, opened by the router), the goal delivered
    /// as a request from its parent, and its loop run to the end.
    pub(crate) async fn adopt_worker(
        &mut self,
        parent: &TaskId,
        started: &Outcome,
    ) -> Result<TaskId, ServeFault> {
        let (task, session) = started_of(started).ok_or(ServeFault::Malformed)?;
        let space = self
            .runtimes
            .get(parent)
            .map(|rt| rt.space.clone())
            .ok_or(ServeFault::UnknownSession)?;
        let open = SessionOpen {
            space,
            agent: AgentRef::Worker { task: task.clone() },
            parent: Some(parent.clone()),
            cwd: None,
        };
        self.adopt(
            &SessionOpened {
                session,
                task: task.clone(),
            },
            &open,
        );
        self.publish();
        self.pull().await?;
        self.drain().await?;
        Ok(task)
    }

    /// The answer of `companion.task.start` as the planner reads it: the new task's id, in the
    /// app's own trusted words, without the session it runs in.
    pub(crate) fn outcome_text(&self, worker: &TaskId, started: Outcome) -> Outcome {
        let label = AppName::parse("org.quire.Companion")
            .map(app_label)
            .unwrap_or_else(|_| docket_planner::planner_label());
        Outcome {
            value: Some(Labelled {
                value: Value::Text(worker.as_str().to_owned()),
                label,
            }),
            ..started
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

/// The task and the session the router answered `companion.task.start` with: a record of both.
fn started_of(outcome: &Outcome) -> Option<(TaskId, prov::SessionId)> {
    let Some(Labelled {
        value: Value::Record(fields),
        ..
    }) = &outcome.value
    else {
        return None;
    };
    let text = |name: &str| {
        fields
            .iter()
            .find(|(k, _)| k.as_str() == name)
            .and_then(|(_, v)| match v {
                Value::Text(t) => Some(t.as_str()),
                _ => None,
            })
    };
    Some((
        TaskId::parse(text("task")?).ok()?,
        prov::SessionId::parse(text("session")?).ok()?,
    ))
}
