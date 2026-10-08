//! The handlers: what a staged request does once it runs. Each forms the call the agent asked
//! for (confining its paths first: a path outside the session's directory never becomes a call),
//! has the router rule on it, and answers the agent as the router ruled. The reply is returned
//! with how the call ends in our record; the loop (`serve`) sends the one and reports the other.
//!
//! The router's refusal is carried as it came: the agent is told only that the call was not
//! allowed, and the record keeps the router's reason.

use super::backend::{AcpBackend, Seams, Staged};
use super::call::{AgentCall, Command, RunFacts};
use super::confine::{Confined, confine, named};
use super::court::{Court, Ruled};
use super::intake::Work;
use super::own_edge::{OwnEdge, claim};
use super::taint::TaintSource;
use super::tool_req::{Asked, tool_req};
use crate::fault;
use agent_client_protocol_schema::v1::{
    CreateTerminalRequest, CreateTerminalResponse, Error, PermissionOptionId, PermissionOptionKind,
    ReadTextFileRequest, ReadTextFileResponse, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, TerminalId, WriteTextFileRequest,
    WriteTextFileResponse,
};
use docket_core::{
    AbsPath, AppRefusal, BreakerTrip, CallRefusal, DenyCode, FailText, Outcome, SandboxState,
    StepEnd, Undoable, Value,
};
use docket_shell::Argv;
use serde::Serialize;
use serde_json::Value as Json;

pub(super) fn ok(value: &impl Serialize) -> Result<Json, Error> {
    serde_json::to_value(value).map_err(|_| fault::internal("could not answer"))
}

/// What running a staged request produced.
pub(super) struct Ran {
    pub reply: Result<Json, Error>,
    pub end: StepEnd,
    /// The router paused the session: the turn ends after the agent is told to stop.
    pub paused: Option<BreakerTrip>,
}

fn not_allowed() -> StepEnd {
    StepEnd::Refused(CallRefusal::Denied(DenyCode::NotAllowed))
}

/// How a call the router refused ends in our record.
fn refused_end(why: CallRefusal) -> StepEnd {
    match why {
        CallRefusal::Unconfirmed(end) => StepEnd::Unconfirmed(end),
        other => StepEnd::Refused(other),
    }
}

fn lost_end() -> StepEnd {
    StepEnd::Refused(CallRefusal::App(AppRefusal::Failed(FailText(
        "the router could not be asked".to_owned(),
    ))))
}

fn done_end(outcome: &Outcome) -> StepEnd {
    StepEnd::Done {
        said: outcome.said.clone(),
        value: None,
        undo: match &outcome.undo {
            Undoable::Journaled(row) => Some(*row),
            Undoable::No | Undoable::Yes(_) => None,
        },
    }
}

fn paused_by(end: &StepEnd) -> Option<BreakerTrip> {
    match end {
        StepEnd::Refused(CallRefusal::Paused(trip)) => Some(*trip),
        _ => None,
    }
}

/// The text a call's value carries.
fn text_of(outcome: &Outcome) -> Option<&str> {
    match outcome.value.as_ref().map(|v| &v.value) {
        Some(Value::Text(t)) => Some(t),
        _ => None,
    }
}

impl<X: Seams> AcpBackend<X> {
    pub(super) async fn run(&mut self, staged: &Staged) -> Ran {
        match &staged.work {
            Work::Read(request) => self.read(staged.n, request).await,
            Work::Write(request) => self.write(staged.n, request).await,
            Work::Permission(request) => self.permission(staged.n, request).await,
            Work::Create { params, request } => {
                self.create(staged.n, params.clone(), request).await
            }
            _ => Ran {
                reply: Err(Error::method_not_found()),
                end: StepEnd::Done {
                    said: None,
                    value: None,
                    undo: None,
                },
                paused: None,
            },
        }
    }

    /// A request that never became a call: refused here, the router not asked. Enough of them in
    /// a row pause the turn.
    fn denied(&mut self, error: Error) -> Ran {
        self.strikes.refused();
        Ran {
            reply: Err(error),
            end: not_allowed(),
            paused: self.strikes.trip(),
        }
    }

    fn for_agent(&self, session: &agent_client_protocol_schema::v1::SessionId) -> bool {
        self.live.as_ref().and_then(|l| l.agent.as_ref()) == Some(session)
    }

    /// The path the agent named, confined to the session's directory, links and secrets included.
    fn confined(&self, text: Option<&str>) -> Option<Confined> {
        let live = self.live.as_ref()?;
        let path = named(&live.cwd, text?).ok()?;
        let real = self.performer.real(&path).ok()?;
        confine(&live.real_cwd, path, real).ok()
    }

    /// Makes `call` and returns how the router ruled.
    pub(super) async fn ask(&mut self, n: u64, call: &AgentCall) -> Ruled {
        let Some(session) = self.live.as_ref().map(|l| l.session.clone()) else {
            return Ruled::Lost;
        };
        let ruled = self.court.call(&session, n, call).await;
        if matches!(ruled, Ruled::Done(_)) {
            self.strikes.let_through();
        }
        ruled
    }

    async fn read(&mut self, n: u64, request: &ReadTextFileRequest) -> Ran {
        if !self.for_agent(&request.session_id) {
            return self.denied(fault::unknown_session());
        }
        let Some(file) = self.confined(request.path.to_str()) else {
            return self.denied(fault::invalid("not a path this session may read"));
        };
        let Some(session) = self.live.as_ref().map(|l| l.session.clone()) else {
            return self.denied(fault::unknown_session());
        };
        let stage =
            self.stage_once(|p| p.stage_read(&session, file.clone(), request.line, request.limit));
        let ruled = self
            .ask(
                n,
                &AgentCall::Read {
                    path: file.path.clone(),
                    stage: stage.clone(),
                },
            )
            .await;
        match ruled {
            Ruled::Done(outcome) => match text_of(&outcome) {
                Some(text) => {
                    // What the file held is untrusted text now in the agent's hands: the router
                    // took the session's taint from the label; this notes which read it was.
                    if let Some(live) = self.live.as_mut() {
                        live.tainted_by
                            .get_or_insert(TaintSource::Served(file.path.clone()));
                    }
                    Ran {
                        reply: ok(&ReadTextFileResponse::new(text)),
                        end: done_end(&outcome),
                        paused: None,
                    }
                }
                None => Ran {
                    reply: Err(fault::invalid("the file could not be read")),
                    end: lost_end(),
                    paused: None,
                },
            },
            other => self.refused(&stage, other, "the read was not allowed"),
        }
    }

    /// A call the router did not let through: the staged request is dropped and the agent told
    /// only that it was not allowed.
    fn refused(&self, stage: &super::call::StageId, ruled: Ruled, why: &str) -> Ran {
        self.performer.drop_stage(stage);
        let end = match ruled {
            Ruled::Refused(refusal) => refused_end(refusal),
            Ruled::Done(_) | Ruled::Lost => lost_end(),
        };
        Ran {
            reply: Err(fault::not_now(why)),
            paused: paused_by(&end),
            end,
        }
    }

    async fn write(&mut self, n: u64, request: &WriteTextFileRequest) -> Ran {
        if !self.for_agent(&request.session_id) {
            return self.denied(fault::unknown_session());
        }
        let Some(file) = self.confined(request.path.to_str()) else {
            return self.denied(fault::invalid("not a path this session may write"));
        };
        let Some(session) = self.live.as_ref().map(|l| l.session.clone()) else {
            return self.denied(fault::unknown_session());
        };
        let lines = u32::try_from(request.content.lines().count()).unwrap_or(u32::MAX);
        let (path, care) = (file.path.clone(), file.care);
        let stage = self.stage_once(|p| p.stage_write(&session, file, request.content.clone()));
        let ruled = self
            .ask(
                n,
                &AgentCall::Write {
                    path,
                    care,
                    lines,
                    stage: stage.clone(),
                },
            )
            .await;
        match ruled {
            Ruled::Done(outcome) => Ran {
                reply: ok(&WriteTextFileResponse::new()),
                end: done_end(&outcome),
                paused: None,
            },
            other => self.refused(&stage, other, "the write was not allowed"),
        }
    }

    async fn permission(&mut self, n: u64, request: &RequestPermissionRequest) -> Ran {
        if !self.for_agent(&request.session_id) {
            return self.denied(fault::unknown_session());
        }
        if let Some(once) = self.own_edge_once(request).await {
            return once;
        }
        let Some(live) = self.live.as_ref() else {
            return self.denied(fault::unknown_session());
        };
        let asked = tool_req(
            &live.cwd,
            &live.real_cwd,
            &|path| self.performer.real(path).ok(),
            &request.tool_call,
        );
        let pick = |kind: PermissionOptionKind| {
            request
                .options
                .iter()
                .find(|o| o.kind == kind)
                .map(|o| o.option_id.clone())
        };
        let answer = |option: Option<PermissionOptionId>| {
            let outcome = option.map_or(RequestPermissionOutcome::Cancelled, |option| {
                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option))
            });
            ok(&RequestPermissionResponse::new(outcome))
        };
        let (reply, end) = match asked {
            Asked::Forbidden => {
                self.strikes.refused();
                (
                    answer(pick(PermissionOptionKind::RejectOnce)),
                    not_allowed(),
                )
            }
            Asked::Ask(ask) => match self.ask(n, &AgentCall::Permission(ask)).await {
                // Our answer is always the "once" option: an "always" the agent offered is never
                // chosen, so its own memory of one never exists; ours is the standing grant in
                // our store. With no matching "once" option the answer is cancelled.
                Ruled::Done(outcome) => (
                    answer(pick(PermissionOptionKind::AllowOnce)),
                    done_end(&outcome),
                ),
                Ruled::Refused(why) => (
                    answer(pick(PermissionOptionKind::RejectOnce)),
                    refused_end(why),
                ),
                Ruled::Lost => (answer(pick(PermissionOptionKind::RejectOnce)), lost_end()),
            },
        };
        Ran {
            reply,
            paused: paused_by(&end).or(self.strikes.trip()),
            end,
        }
    }

    /// A request for a call to our own edge, answered with the agent's "once" option and no
    /// ruling: the call is ruled when it reaches the edge. Never the "always" option. With no
    /// "once" option the request is left to the usual path.
    async fn own_edge_once(&mut self, request: &RequestPermissionRequest) -> Option<Ran> {
        let own = claim(&request.tool_call)?;
        if let OwnEdge::Tool(tool) = &own {
            let registry = self.court.registry().await?;
            actions_tools::find(&registry, tool)?;
        }
        let once = request
            .options
            .iter()
            .find(|o| o.kind == PermissionOptionKind::AllowOnce)?;
        let outcome = SelectedPermissionOutcome::new(once.option_id.clone());
        let reply = ok(&RequestPermissionResponse::new(
            RequestPermissionOutcome::Selected(outcome),
        ));
        self.ready
            .extend(super::reported::door_opened(&mut self.next_call));
        Some(Ran {
            reply,
            end: StepEnd::Done {
                said: None,
                value: None,
                undo: None,
            },
            paused: None,
        })
    }

    async fn create(&mut self, n: u64, params: Json, request: &CreateTerminalRequest) -> Ran {
        if !self.for_agent(&request.session_id) {
            return self.denied(fault::unknown_session());
        }
        let Some(live) = self.live.as_ref() else {
            return self.denied(fault::unknown_session());
        };
        let (scope, session) = (live.cwd.clone(), live.session.clone());
        let Some(argv) = Argv::new(&request.command, &request.args) else {
            return self.denied(fault::invalid("the command is empty or holds a NUL"));
        };
        let cwd = match &request.cwd {
            Some(path) => AbsPath::parse(&path.to_string_lossy()).ok(),
            None => Some(scope.clone()),
        };
        let Some(cwd) = cwd.filter(|c| scope.covers(c) == docket_core::Cover::Covers) else {
            return self.denied(fault::invalid(
                "the working directory is outside the session",
            ));
        };
        // We never run unsandboxed: a command that cannot be confined is refused before anyone
        // is asked, with the reason; there is nothing for an approval to unlock.
        if let SandboxState::Cannot(why) = self.performer.check(&cwd) {
            return self.denied(fault::not_now(&format!("cannot sandbox: {why}")));
        }
        let line = argv.line();
        let stage = self.stage_once(|p| p.stage_run(&session, line.clone(), cwd.clone(), params));
        let (derives, network) = self.performer.exec_facts(&session, argv.words(), &cwd);
        let command = Command { line, cwd };
        let ruled = self
            .ask(
                n,
                &AgentCall::Run {
                    command,
                    facts: RunFacts { derives, network },
                    stage: stage.clone(),
                },
            )
            .await;
        match ruled {
            Ruled::Done(outcome) => match text_of(&outcome) {
                Some(id) => Ran {
                    reply: ok(&CreateTerminalResponse::new(TerminalId::new(id.to_owned()))),
                    end: done_end(&outcome),
                    paused: None,
                },
                None => Ran {
                    reply: Err(fault::internal("the terminal has no name")),
                    end: lost_end(),
                    paused: None,
                },
            },
            other => self.refused(&stage, other, "the command was not allowed"),
        }
    }
}
