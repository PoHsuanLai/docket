//! Opening and closing sessions, and recording the person's turns. A task is its own session:
//! its taint, its budget and its policy are its own, and a child's policy is never wider than
//! its parent's.

use crate::router::Router;
use crate::seams::{Clock, EventSink, Seams};
use crate::session::{SessionEvent, session_step};
use crate::state::SessionRecord;
use crate::tasks::{TaskRecord, TaskState, child_policy};
use almanac_core::{EpisodeId, EpisodeKind, EpisodeOutcome};
use docket_core::{
    AuditRecord, CallerId, CallerRole, IntentsReply, Reveal, SessionOpen, SessionOpened,
    TaskLedger, TurnId, TurnIn, TurnSource, UserTurn, WireRefusal, close,
};
use porter_core::AppName;
use prov::{Actor, AgentRef, AgentRole, ReportStatus, SessionId, TaskId};

fn refuse(why: WireRefusal) -> IntentsReply {
    IntentsReply::Refused(why)
}

impl<S: Seams> Router<S> {
    /// `.Session.Open`.
    ///
    /// The computer-use daemon opens the session of its own run and nothing else; a run has one
    /// session, whoever opened it, so a second open of the same run is refused.
    pub(crate) fn session_open(
        &self,
        caller: &CallerId,
        role: CallerRole,
        open: SessionOpen,
    ) -> IntentsReply {
        if role == CallerRole::Cua && !matches!(open.agent, AgentRef::Cua { .. }) {
            return refuse(WireRefusal::NotAllowed);
        }
        if let AgentRef::Cua { run } = &open.agent
            && self.locked().sessions.values().any(|r| {
                matches!(&r.actor, Actor::Companion { role: AgentRole::Cua { run: held }, .. } if held == run)
            })
        {
            return refuse(WireRefusal::Malformed);
        }
        self.open_session(&caller.app.name, open)
    }

    /// Opens a session for `opener`: the one body of `.Session.Open`, which the hosted
    /// `org.quire.Companion` provider also uses to start a worker.
    pub(crate) fn open_session(&self, opener: &AppName, open: SessionOpen) -> IntentsReply {
        let now = self.seams.clock().now();
        let mut st = self.locked();
        let n = st.mint();
        let (Ok(session), Ok(minted)) = (
            SessionId::parse(&format!("s-{n}")),
            TaskId::parse(&format!("t-{n}")),
        ) else {
            return refuse(WireRefusal::Malformed);
        };
        let task = match &open.agent {
            AgentRef::Worker { task } => task.clone(),
            AgentRef::Companion | AgentRef::Cua { .. } | AgentRef::User => minted,
        };
        if st.tasks.get(&task).is_some() {
            return refuse(WireRefusal::Malformed);
        }
        let actor = match &open.agent {
            AgentRef::Companion => Actor::Companion {
                session: session.clone(),
                role: AgentRole::Planner,
            },
            AgentRef::Worker { task } => Actor::Companion {
                session: session.clone(),
                role: AgentRole::Worker { task: task.clone() },
            },
            AgentRef::Cua { run } => Actor::Companion {
                session: session.clone(),
                role: AgentRole::Cua { run: run.clone() },
            },
            AgentRef::User => Actor::User {
                via: opener.clone(),
            },
        };
        let mut record =
            SessionRecord::new(task.clone(), actor, opener.clone(), open.space.clone(), now);
        let parent_episode = open
            .parent
            .as_ref()
            .and_then(|p| EpisodeId::parse(p.as_str()).ok());
        if let Some(parent) = &open.parent
            && let Some(inherited) = st
                .sessions
                .values()
                .find(|r| &r.task == parent)
                .and_then(|r| r.policy.clone())
        {
            let mut asked = inherited.clone();
            asked.task = task.clone();
            asked.space = open.space.clone();
            record.policy = Some(child_policy(&inherited, &asked));
        }
        st.sessions.insert(session.clone(), record);
        st.tasks.insert(TaskRecord {
            task: task.clone(),
            session: session.clone(),
            agent: open.agent.clone(),
            parent: open.parent.clone(),
            space: open.space.clone(),
            state: TaskState::Working,
            goal: Reveal::Plain(String::new()),
            last: None,
            ledger: TaskLedger {
                task: task.clone(),
                agent: open.agent,
                parent: parent_episode,
                space: open.space,
                started: now,
                asked: vec![],
                steps: vec![],
                touched: vec![],
                results: vec![],
            },
        });
        IntentsReply::SessionOpened(SessionOpened { session, task })
    }

    /// `.Session.Close`: the task ends and leaves its skeleton.
    pub(crate) fn session_close(
        &self,
        caller: &CallerId,
        role: CallerRole,
        id: &SessionId,
    ) -> IntentsReply {
        let now = self.seams.clock().now();
        let mut st = self.locked();
        let Some(record) = st.sessions.get_mut(id) else {
            return refuse(WireRefusal::NoSuchSession);
        };
        if matches!(role, CallerRole::Field | CallerRole::Cua) && record.opener != caller.app.name {
            return refuse(WireRefusal::NotAllowed);
        }
        record.state = session_step(record.state, SessionEvent::Close).0;
        let task = record.task.clone();
        let policy_ended = record.policy.is_some();
        let Some(t) = st.tasks.get_mut(&task) else {
            return IntentsReply::Done;
        };
        let outcome = match t.state {
            TaskState::Ended(_) => None,
            _ => {
                t.state = TaskState::Ended(ReportStatus::Done);
                Some(EpisodeOutcome::Done)
            }
        };
        // A session nobody used (no turn, no step) leaves no episode: there is nothing to remember.
        let episode = outcome
            .and_then(|o| close(&t.ledger, EpisodeKind::Task, now, o))
            .filter(|e| !(e.skeleton.asked.is_empty() && e.skeleton.steps.is_empty()));
        drop(st);
        if policy_ended {
            self.seams.sink().append(AuditRecord::TaskPolicy {
                at: now,
                task,
                state: docket_core::TaskPolicyState::Expired,
                change: docket_core::PolicyChangeKind::Narrows,
            });
        }
        if let Some(episode) = episode {
            self.seams
                .sink()
                .append(AuditRecord::Episode(Box::new(episode)));
        }
        IntentsReply::Done
    }

    /// Records the person's words, resuming a paused session. The words are the person's:
    /// only the launcher and a prompt field reach here (the role table).
    pub(crate) fn record_turn(
        &self,
        caller: &CallerId,
        role: CallerRole,
        id: &SessionId,
        turn: TurnIn,
    ) -> Result<UserTurn, WireRefusal> {
        let now = self.seams.clock().now();
        let mut st = self.locked();
        let number = u64::from(st.mint());
        let record = st.sessions.get_mut(id).ok_or(WireRefusal::NoSuchSession)?;
        if role == CallerRole::Field && record.opener != caller.app.name {
            return Err(WireRefusal::NotAllowed);
        }
        let from = match role {
            CallerRole::Field => TurnSource::Field(caller.app.name.clone()),
            _ => TurnSource::Launcher,
        };
        let recorded = UserTurn {
            id: TurnId(number),
            text: turn.text,
            at: now,
            from,
            via: turn.via,
        };
        let (state, _) = session_step(record.state, SessionEvent::UserTurn);
        record.state = state;
        record.turns.push(recorded.clone());
        let task = record.task.clone();
        if let Some(t) = st.tasks.get_mut(&task) {
            if matches!(&t.goal, Reveal::Plain(g) if g.is_empty()) {
                t.goal = Reveal::Plain(recorded.text.clone());
            }
            if matches!(t.state, TaskState::NeedsYou | TaskState::Paused) {
                t.state = TaskState::Working;
            }
            t.ledger.asked.push(recorded.clone());
        }
        Ok(recorded)
    }
}
