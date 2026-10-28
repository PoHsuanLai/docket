//! The two actions of `org.quire.Companion`, which intentd hosts as a built-in provider: starting
//! a worker task and messaging one. They live here because both are the router's own business:
//! a worker is a child session under the spawner's task policy (never wider), and a message goes
//! through the same stamping, labelling and delivery as `.Message.Send`.
//!
//! The spawner's words are a model's, so the goal is held as a handle in the spawning session's
//! table (`TaskRecord::goal`) and reaches the worker as a request message whose label is the
//! planner's; nothing here reads it as an instruction.

use crate::companion_names::{HELPER_POST_JOB, HELPER_PROPOSE, LIMIT_ADD, ROUTINE_PROPOSE};
use crate::handles::HandleValue;
use crate::labels::{app_label, model_label};
use crate::router::Router;
use crate::seams::Seams;
use crate::spoken::whole_in;
use companion_model::helper::{GoalOrigin, JobRefusal, PlannerSaw};
use docket_core::EventSink;
use docket_core::{
    AppRefusal, AuditRecord, DraftPart, FROM_CHECKED_BRIEF, FailText, Invocation, MessageDraft,
    Outcome, Preview, Reveal, SendRefusal, Undoable,
};
use docket_permissions_core::{COMPANION_APP, Saw};
use docket_vocab::{CallerRole, ChoiceId, Clock, LabelText, ParamName, TargetValue, Value};
use docket_wire::{CallerId, IntentsReply, SessionOpen, SessionOpened, TaskStart, WireRefusal};
use porter_core::{AppId, AppName, Isolation};
use prov::{
    Actor, Address, AgentRef, AgentRole, EntityId, EntityKey, EntityKind, Integrity, Labelled,
    MessageKind, MessageText, ModelRole, SessionId, Source, TaskId,
};
use std::collections::BTreeMap;

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

fn nothing_back(words: &str) -> Outcome {
    Outcome {
        value: None,
        said: said(words),
        show: Preview::None,
        undo: Undoable::No,
        follow: docket_core::Follow::Nothing,
    }
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
            LIMIT_ADD => self.limit_add(&inv),
            HELPER_POST_JOB => self.helper_post_job(&inv),
            HELPER_PROPOSE => self.helper_propose(&inv),
            ROUTINE_PROPOSE => Self::routine_propose(&inv),
            _ => Err(AppRefusal::Unsupported),
        }
    }

    /// The words of `param`, which must be a planner's own: written by a planner that had read
    /// nothing untrusted, not taken from a handle. The labels are the router's, never the
    /// planner's: a goal or a role it derived from content it read is data, and a helper is never
    /// told to obey data.
    fn clean_words(
        &self,
        inv: &Invocation,
        session: &SessionId,
        param: &str,
        why: JobRefusal,
    ) -> Result<String, AppRefusal> {
        let text = text_arg(inv, param)?;
        let written_by_a_clean_planner = self
            .locked()
            .sessions
            .get(session)
            .is_some_and(|record| record.saw.untrusted == Saw::NotSeen)
            && inv
                .args
                .get(&ParamName::parse(param).map_err(|_| AppRefusal::Unsupported)?)
                .is_some_and(|arg| {
                    arg.label.sources.iter().all(|source| {
                        matches!(source, Source::Model(ModelRole::Planner) | Source::User)
                    })
                });
        if written_by_a_clean_planner {
            Ok(text)
        } else {
            Err(failed(&why.plain()))
        }
    }

    /// Whether the goal lies, as whole tokens, inside a brief the person confirmed in this
    /// session, and carries nothing but their label. The router made that label (see `trace`),
    /// never the planner.
    fn in_confirmed_brief(&self, inv: &Invocation, session: &SessionId) -> bool {
        let Ok(text) = text_arg(inv, "goal") else {
            return false;
        };
        let theirs = inv.args.get(&param("goal")).is_some_and(|arg| {
            arg.label.integrity == Integrity::Trusted
                && !arg.label.sources.is_empty()
                && arg.label.sources.iter().all(|s| *s == Source::User)
        });
        theirs
            && self.locked().sessions.get(session).is_some_and(|record| {
                record
                    .briefs
                    .iter()
                    .any(|brief| whole_in(brief.text.as_str(), &text))
            })
    }

    /// Where the goal of a job came from: a brief the person checked, or a planner that read
    /// nothing untrusted. Anything else is refused.
    fn goal_origin(&self, inv: &Invocation, session: &SessionId) -> Result<GoalOrigin, AppRefusal> {
        if self.in_confirmed_brief(inv, session) {
            return Ok(GoalOrigin::ConfirmedBrief);
        }
        self.clean_words(inv, session, "goal", JobRefusal::UntrustedGoal)
            .map(|_| GoalOrigin::Planner(PlannerSaw::Nothing))
    }

    fn helper_post_job(&self, inv: &Invocation) -> Result<Outcome, AppRefusal> {
        let session = spawner(inv)?;
        text_arg(inv, "helper")?;
        let origin = self.goal_origin(inv, session)?;
        let mut outcome = nothing_back("Gave the job to a helper");
        if origin == GoalOrigin::ConfirmedBrief {
            // The host that keeps the helpers reads this to record where the goal came from.
            outcome.value = ChoiceId::parse(FROM_CHECKED_BRIEF)
                .ok()
                .map(|mark| Labelled {
                    value: Value::Choice(mark),
                    label: app_label(&name()),
                });
        }
        Ok(outcome)
    }

    fn helper_propose(&self, inv: &Invocation) -> Result<Outcome, AppRefusal> {
        let session = spawner(inv)?;
        let untrusted = JobRefusal::UntrustedGoal;
        self.clean_words(inv, session, "name", untrusted.clone())?;
        self.clean_words(inv, session, "role", untrusted)?;
        text_arg(inv, "apps")?;
        Ok(nothing_back("Proposed a helper"))
    }

    /// A routine proposed: nothing is made here, the person accepts it on a sheet. A planner that
    /// read untrusted text may still propose, since nothing runs before they say yes; its `why`
    /// is carried as data, and so are the goal and the time.
    fn routine_propose(inv: &Invocation) -> Result<Outcome, AppRefusal> {
        spawner(inv)?;
        text_arg(inv, "goal")?;
        text_arg(inv, "when")?;
        text_arg(inv, "why")?;
        Ok(nothing_back("Proposed a routine"))
    }

    fn task_start(&self, inv: &Invocation) -> Result<Outcome, AppRefusal> {
        let session = spawner(inv)?;
        // A task does its own goal: only the person's assistant hands out work, so a worker
        // that started tasks would start itself again, one nested run inside the last.
        if matches!(
            &inv.actor,
            Actor::Companion {
                role: AgentRole::Worker { .. },
                ..
            }
        ) {
            return Err(AppRefusal::Unsupported);
        }
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
                external: None,
                helper: None,
                acts_for: None,
            },
        );
        let IntentsReply::SessionOpened(SessionOpened {
            task,
            session: worker_session,
            ..
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
        // The planner is given the task as a thing of kind `companion.task`, a handle it can pass
        // as the target of `companion.task.message`; the host that runs the worker is given the
        // task and the session it runs in: acting as the child needs them.
        let handle = {
            let thing = task_entity(&task).ok_or_else(|| failed("no task id"))?;
            let mut st = self.locked();
            let record = st
                .sessions
                .get_mut(session)
                .ok_or_else(|| failed("the session is gone"))?;
            record
                .know([thing.clone()])
                .map_err(|_| failed("too many things are held"))?;
            record
                .handles
                .try_mint_entity(thing, app_label(&name()))
                .map_err(|_| failed("too many things are held"))?
        };
        let answer = BTreeMap::from([
            (param("handle"), Value::Handle(handle)),
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

/// The thing of kind `companion.task` that stands for `task`.
fn task_entity(task: &TaskId) -> Option<EntityId> {
    Some(EntityId {
        app: name(),
        kind: EntityKind::parse(TASK_KIND).ok()?,
        key: EntityKey::parse(task.as_str()).ok()?,
    })
}

/// The task an entity of kind `companion.task` names.
fn task_of(entity: &EntityId) -> Option<TaskId> {
    (entity.app == name() && entity.kind.as_str() == TASK_KIND)
        .then(|| TaskId::parse(entity.key.as_str()).ok())
        .flatten()
}
