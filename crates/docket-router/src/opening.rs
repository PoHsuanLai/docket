//! Opening and closing sessions, and recording the person's turns. A task is its own session:
//! its taint, its budget and its policy are its own, and a child's policy is never wider than
//! its parent's.

use crate::router::Router;
use crate::seams::Seams;
use almanac_core::EpisodeId;
use companion_model::helper::SeatPlace;
use docket_context::session::SessionEvent;
use docket_context::state::SessionRecord;
use docket_context::tasks::{TaskRecord, TaskState, child_policy, end_task};
use docket_context::wal::Wal;
use docket_core::EventSink;
use docket_core::{AuditRecord, Reveal, Rewind, TurnIn, UserTurn};
use docket_permissions_core::intersection;
use docket_session::{BackendKind, Opening, SessionEntry, Taint};
use docket_vocab::{Caller, CallerRole, Clock, TurnId};
use docket_wire::{CallerId, IntentsReply, SessionOpen, SessionOpened, TaskLedger, WireRefusal};
use porter_core::AppName;
use prov::{Actor, AgentRef, AgentRole, SessionId, TaskId};

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
        // The host of an external agent opens that agent's session and nothing else, and says
        // which program it is; nobody else may say it.
        let external = match (role, &open.external) {
            (CallerRole::AcpAgent, Some(external)) if open.agent == AgentRef::Companion => {
                Some(external.clone())
            }
            (CallerRole::AcpAgent, _) => return refuse(WireRefusal::NotAllowed),
            _ => None,
        };
        if let AgentRef::Cua { run } = &open.agent
            && self.locked().sessions.values().any(|r| {
                matches!(&r.actor, Actor::Companion { role: AgentRole::Cua { run: held }, .. } if held == run)
            })
        {
            return refuse(WireRefusal::Malformed);
        }
        // Only the companion may say which terminal a conversation came from: it saw the cgroup.
        let started_from = open
            .started_from
            .clone()
            .filter(|_| role == CallerRole::Companion);
        // Only the companion may say a session is a helper's job: a seat from anyone else is
        // ignored, so it can neither narrow nor protect anything it should not.
        let helper = open
            .helper
            .clone()
            .filter(|_| role == CallerRole::Companion);
        // Only the computer-use daemon may say which conversation a run works for, and only for
        // the run's own session: from anyone else it is ignored, so no session can borrow a looser
        // conversation's mode.
        let acts_for = open
            .acts_for
            .clone()
            .filter(|_| role == CallerRole::Cua && matches!(open.agent, AgentRef::Cua { .. }));
        self.open_session(
            &caller.app.name,
            SessionOpen {
                started_from,
                external,
                helper,
                acts_for,
                ..open
            },
        )
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
        let actor = match &open.helper {
            Some(seat) => Actor::Companion {
                session: session.clone(),
                role: AgentRole::Helper {
                    helper: seat.helper.clone(),
                    task: task.clone(),
                },
            },
            None => {
                docket_context::who::actor_of(&open.agent, open.external.as_ref(), &session, opener)
            }
        };
        let mut record =
            SessionRecord::new(task.clone(), actor, opener.clone(), open.space.clone(), now);
        // A session that works for a conversation (a worker, a helper's job, a screen run whose
        // opener named the task that started it) follows that conversation's mode; any other
        // starts in the person's start mode.
        // A screen run names the conversation it works for itself (the daemon knows the session
        // that started the run, not its task; `session_open` kept it only from that daemon). It
        // must be a conversation the router holds, in the same Space.
        let acts_for = open
            .acts_for
            .as_ref()
            .filter(|c| st.sessions.get(*c).is_some_and(|r| r.space == open.space))
            .cloned();
        let parent_session = acts_for.or_else(|| {
            open.parent.as_ref().and_then(|p| {
                st.sessions
                    .iter()
                    .find(|(_, r)| &r.task == p)
                    .map(|(id, _)| id.clone())
            })
        });
        record.mode = match &parent_session {
            Some(id) => st.mode_of(id).unwrap_or_else(|| self.start_mode()),
            None => self.start_mode(),
        };
        record.parent = parent_session;
        record.external = open.external.clone();
        record.cwd = open.cwd.clone();
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
        // A helper's job never has more than the helper's reach, whatever its parent allows.
        if let Some(seat) = &open.helper {
            record.policy = record.policy.take().map(|p| intersection(&p, &seat.reach));
        }
        record.helper = open.helper.clone();
        record.wal = Wal::on(Taint::Clean);
        record.wal.note(SessionEntry::Opened(Opening {
            task: task.clone(),
            space: open.space.clone(),
            opener: Some(opener.clone()),
            agent: Some(open.agent.clone()),
            backend: open
                .external
                .as_ref()
                .map_or(BackendKind::Native, |e| BackendKind::Acp(e.program.clone())),
            parent: open.parent.clone(),
            forked_from: None,
            cwd: open.cwd.clone(),
            started_from: open.started_from.clone(),
            label: open.external.as_ref().and_then(|e| e.label.clone()),
            rewind: open.external.as_ref().map_or(Rewind::Docket, |e| e.rewind),
        }));
        // A conversation's seat is kept in the log, so a restored session is still the helper's.
        // A job's is not: it is the companion's to give again when the job is asked again.
        if let Some(seat) = open
            .helper
            .as_ref()
            .filter(|seat| seat.place == SeatPlace::Conversation)
        {
            record.wal.note(SessionEntry::Seat(seat.clone()));
        }
        if let Some(policy) = &record.policy {
            record.wal.note(SessionEntry::Policy(policy.clone()));
        }
        let mode = record.mode;
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
        IntentsReply::SessionOpened(SessionOpened {
            session,
            task,
            mode,
        })
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
        if matches!(
            role,
            CallerRole::Field | CallerRole::Cua | CallerRole::Editor | CallerRole::AcpAgent
        ) && record.opener != caller.app.name
        {
            return refuse(WireRefusal::NotAllowed);
        }
        record.apply(SessionEvent::Close);
        let task = record.task.clone();
        let policy_ended = record.policy.is_some();
        st.running.remove(id);
        let Some(t) = st.tasks.get_mut(&task) else {
            return IntentsReply::Done;
        };
        let episode = end_task(t, now);
        drop(st);
        if policy_ended {
            self.seams.sink().append(AuditRecord::TaskPolicy {
                at: now,
                task,
                state: docket_permissions_core::TaskPolicyState::Expired,
                change: docket_permissions_core::PolicyChangeKind::Narrows,
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
        if matches!(
            role,
            CallerRole::Field | CallerRole::Editor | CallerRole::AcpAgent
        ) && record.opener != caller.app.name
            && !record.is_conversation_for(role)
        {
            return Err(WireRefusal::NotAllowed);
        }
        let from = Caller::of(role, &caller.app.name);
        // The mode travels with the turn, but only the shell can say it: the same words from an
        // editor, a terminal or an agent's host change nothing.
        let mode = turn
            .mode
            .filter(|_| role == CallerRole::Launcher)
            .map(|wanted| self.floor.clamp_mode(wanted));
        if let Some(mode) = mode {
            record.mode = mode;
        }
        let recorded = UserTurn {
            id: TurnId(number),
            text: turn.text,
            at: now,
            from,
            via: turn.via,
            mode,
        };
        record.apply(SessionEvent::UserTurn);
        // A new turn ends the one-use yeses of the last.
        record.approvals.clear();
        record.wal.note(SessionEntry::Turn(recorded.clone()));
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
