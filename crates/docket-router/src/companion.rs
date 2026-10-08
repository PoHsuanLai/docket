//! The two actions of `org.quire.Companion`, which intentd hosts as a built-in provider: starting
//! a worker task and messaging one. They live here because both are the router's own business:
//! a worker is a child session under the spawner's task policy (never wider), and a message goes
//! through the same stamping, labelling and delivery as `.Message.Send`.
//!
//! The spawner's words are a model's, so the goal is held as a handle in the spawning session's
//! table (`TaskRecord::goal`) and reaches the worker as a request message whose label is the
//! planner's; nothing here reads it as an instruction.

use crate::handles::HandleValue;
use crate::labels::{app_label, model_label};
use crate::router::Router;
use crate::seams::{Clock, EventSink, Seams};
use docket_core::{
    AppRefusal, AuditRecord, CallerId, CallerRole, DraftPart, FailText, IntentsReply, Invocation,
    LabelText, MessageDraft, Outcome, ParamName, Preview, Reveal, SendRefusal, SessionOpen,
    SessionOpened, TargetValue, TaskStart, Undoable, Value, WireRefusal,
};
use porter_core::{AppId, AppName, Isolation};
use prov::{
    Actor, Address, AgentRef, EntityId, Labelled, MessageKind, MessageText, SessionId, Source,
    TaskId,
};
use std::collections::BTreeMap;

/// The name the built-in provider answers to.
pub const COMPANION_APP: &str = "org.quire.Companion";

/// The kind of thing a task is.
const TASK_KIND: &str = "companion.task";

fn failed(why: &str) -> AppRefusal {
    AppRefusal::Failed(FailText(why.to_owned()))
}

fn name() -> AppName {
    AppName::parse(COMPANION_APP).expect("`org.quire.Companion` is a valid app name")
}

/// The caller the router speaks as when the provider sends on a session's behalf: the
/// companion role under the built-in's own name. `message_send` takes the sender from the
/// session, never from this.
fn companion() -> CallerId {
    CallerId {
        app: AppId {
            name: name(),
            isolation: Isolation::InProcess,
        },
        roles: std::collections::BTreeSet::from([CallerRole::Companion]),
    }
}

fn text_arg(inv: &Invocation, param: &str) -> Result<String, AppRefusal> {
    let name = ParamName::parse(param).map_err(|_| AppRefusal::Unsupported)?;
    match inv.args.get(&name).map(|a| &a.value) {
        Some(Value::Text(t)) if !t.is_empty() => Ok(t.clone()),
        Some(Value::Text(_)) | None => Err(AppRefusal::NeedsParam {
            param: name,
            options: Vec::new(),
        }),
        Some(_) => Err(failed("that parameter is text")),
    }
}

/// The session an invocation acts in: only a companion's own session may start or message a
/// task.
fn spawner(inv: &Invocation) -> Result<&SessionId, AppRefusal> {
    match &inv.actor {
        Actor::Companion { session, .. } => Ok(session),
        _ => Err(AppRefusal::Unsupported),
    }
}

fn param(name: &str) -> ParamName {
    ParamName::parse(name).expect("a fixed parameter name is valid")
}

fn said(text: &str) -> Option<LabelText> {
    LabelText::parse(text).ok()
}

fn refused_send(why: IntentsReply) -> AppRefusal {
    match why {
        IntentsReply::Refused(WireRefusal::Send(SendRefusal::NoRecipient)) => {
            failed("there is no such task")
        }
        IntentsReply::Refused(WireRefusal::Send(SendRefusal::Halted)) => AppRefusal::Busy,
        _ => failed("the message was not delivered"),
    }
}

impl<S: Seams> Router<S> {
    /// Performs `companion.task.start` or `companion.task.message` for the hosted
    /// `org.quire.Companion` provider. Anything else is unsupported.
    pub fn companion_perform(&self, inv: Invocation) -> Result<Outcome, AppRefusal> {
        match inv.action.as_str() {
            "companion.task.start" => self.task_start(&inv),
            "companion.task.message" => self.task_message(&inv),
            crate::skills::SKILL_LOAD => self.skill_load(&inv),
            _ => Err(AppRefusal::Unsupported),
        }
    }

    fn task_start(&self, inv: &Invocation) -> Result<Outcome, AppRefusal> {
        let session = spawner(inv)?;
        let start = TaskStart::from_args(&inv.args).map_err(|_| failed("the goal is missing"))?;
        let (parent, worker) = {
            let mut st = self.locked();
            let parent = st
                .sessions
                .get(session)
                .map(|r| r.task.clone())
                .ok_or_else(|| failed("the session is gone"))?;
            let n = st.mint();
            let worker = TaskId::parse(&format!("t-{n}")).map_err(|_| failed("no task id"))?;
            (parent, worker)
        };
        let opened = self.open_session(
            &name(),
            SessionOpen {
                space: inv.space.clone(),
                agent: AgentRef::Worker {
                    task: worker.clone(),
                },
                parent: Some(parent.clone()),
                cwd: None,
                started_from: None,
            },
        );
        let IntentsReply::SessionOpened(SessionOpened {
            task,
            session: worker_session,
        }) = opened
        else {
            return Err(failed("the task could not be opened"));
        };
        let goal = {
            let mut st = self.locked();
            let label = model_label();
            let handle = st
                .sessions
                .get_mut(session)
                .map(|r| {
                    r.handles.mint(
                        Labelled {
                            value: HandleValue::Text(start.goal.as_str().to_owned()),
                            label,
                        },
                        Source::Model(prov::ModelRole::Planner),
                    )
                })
                .ok_or_else(|| failed("the session is gone"))?;
            if let Some(t) = st.tasks.get_mut(&task) {
                t.goal = Reveal::Handle(handle);
            }
            handle
        };
        let sent = self.message_send(
            &companion(),
            CallerRole::Companion,
            session,
            MessageDraft {
                to: Address::new(AgentRef::Worker { task: task.clone() }, inv.space.clone()),
                thread: None,
                in_reply_to: None,
                kind: MessageKind::Request,
                parts: vec![DraftPart::Handle(goal)],
            },
        );
        if !matches!(sent, IntentsReply::Delivered(_)) {
            return Err(refused_send(sent));
        }
        self.seams.sink().append(AuditRecord::TaskStarted {
            at: self.seams.clock().now(),
            task: task.clone(),
            agent: AgentRef::Worker { task: task.clone() },
            parent: Some(parent),
            space: inv.space.clone(),
            by_call: inv.call,
        });
        // The spawner is given the task and the session it runs in: acting as the child needs it.
        let answer = BTreeMap::from([
            (param("task"), Value::Text(task.as_str().to_owned())),
            (
                param("session"),
                Value::Text(worker_session.as_str().to_owned()),
            ),
        ]);
        Ok(Outcome {
            value: Some(Labelled {
                value: Value::Record(answer),
                label: app_label(&name()),
            }),
            said: said("Started a task"),
            show: Preview::None,
            undo: Undoable::No,
            follow: docket_core::Follow::Nothing,
        })
    }

    fn task_message(&self, inv: &Invocation) -> Result<Outcome, AppRefusal> {
        let session = spawner(inv)?;
        let text = text_arg(inv, "text")?;
        let TargetValue::Entities(targets) = &inv.target else {
            return Err(AppRefusal::Unsupported);
        };
        let [target] = targets.as_slice() else {
            return Err(AppRefusal::Unsupported);
        };
        let task = task_of(target).ok_or_else(|| AppRefusal::NotFound(target.clone()))?;
        // The agent is the one the task record names: a worker, or a run of the computer-use
        // daemon, whichever the task is.
        let (agent, space) = self
            .locked()
            .tasks
            .get(&task)
            .map(|t| (t.agent.clone(), t.space.clone()))
            .ok_or_else(|| AppRefusal::NotFound(target.clone()))?;
        let sent = self.message_send(
            &companion(),
            CallerRole::Companion,
            session,
            MessageDraft {
                to: Address::new(agent, space),
                thread: None,
                in_reply_to: None,
                kind: MessageKind::Request,
                parts: vec![DraftPart::Text(MessageText::new(text))],
            },
        );
        match sent {
            IntentsReply::Delivered(_) => Ok(Outcome {
                value: None,
                said: said("Sent the message"),
                show: Preview::None,
                undo: Undoable::No,
                follow: docket_core::Follow::Nothing,
            }),
            other => Err(refused_send(other)),
        }
    }
}

/// The task an entity of kind `companion.task` names.
fn task_of(entity: &EntityId) -> Option<TaskId> {
    (entity.app == name() && entity.kind.as_str() == TASK_KIND)
        .then(|| TaskId::parse(entity.key.as_str()).ok())
        .flatten()
}
