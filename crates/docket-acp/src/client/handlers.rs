//! The handlers: what a staged request does once it runs. Each returns the reply for the agent
//! and how the call ends in our record; the loop (`serve`) sends the one and reports the other.

use super::backend::{AcpBackend, Seams, Staged};
use super::confine::{confine, named};
use super::files::{FileFault, Files};
use super::gate::{Audit, Basis, Ruling, Why};
use super::intake::Work;
use super::tool_req::tool_req;
use crate::fault;
use crate::server::Ticks;
use agent_client_protocol_schema::v1::{
    Error, PermissionOptionKind, ReadTextFileRequest, ReadTextFileResponse,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, WriteTextFileRequest, WriteTextFileResponse,
};
use docket_core::{AbsPath, AppRefusal, CallRefusal, ConfirmEnd, DenyCode, FailText, StepEnd};
use serde::Serialize;
use serde_json::Value;

/// A write made for the agent: the old text, kept for the undo journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoNote {
    /// The file.
    pub path: AbsPath,
    /// What was there; `None` if the write created it.
    pub before: Option<String>,
}

/// The most undo notes kept in memory per connection.
const UNDO_KEEP: usize = 256;

pub(super) fn ok(value: &impl Serialize) -> Result<Value, Error> {
    serde_json::to_value(value).map_err(|_| fault::internal("could not answer"))
}

/// What a refused call is called in the record, and what the agent is told (nothing specific).
fn refused_end(why: Why, trip: Option<docket_core::BreakerTrip>) -> StepEnd {
    match why {
        Why::Declined => StepEnd::Unconfirmed(ConfirmEnd::Refused),
        Why::Breaker => match trip {
            Some(trip) => StepEnd::Refused(CallRefusal::Paused(trip)),
            None => StepEnd::Refused(CallRefusal::Denied(DenyCode::NotAllowed)),
        },
        Why::Forbidden | Why::Reviewer => {
            StepEnd::Refused(CallRefusal::Denied(DenyCode::NotAllowed))
        }
    }
}

fn file_end(fault: FileFault) -> StepEnd {
    StepEnd::Refused(CallRefusal::App(AppRefusal::Failed(FailText(format!(
        "the file call failed: {fault}"
    )))))
}

fn done() -> StepEnd {
    StepEnd::Done {
        said: None,
        value: None,
        undo: None,
    }
}

/// What running a staged request produced.
pub(super) struct Ran {
    pub reply: Result<Value, Error>,
    pub end: StepEnd,
}

fn lines_of(content: &str, from: Option<u32>, limit: Option<u32>) -> String {
    let skip = from.map_or(0, |n| n.saturating_sub(1)) as usize;
    let take = limit.map_or(usize::MAX, |n| n as usize);
    if skip == 0 && take == usize::MAX {
        return content.to_owned();
    }
    content
        .split_inclusive('\n')
        .skip(skip)
        .take(take)
        .collect()
}

impl<X: Seams> AcpBackend<X> {
    pub(super) async fn run(&mut self, staged: &Staged) -> Ran {
        match &staged.work {
            Work::Read(request) => self.read(request),
            Work::Write(request) => self.write(staged.n, request).await,
            Work::Permission(request) => self.permission(staged.n, request).await,
            Work::Create { params, request } => {
                self.create(staged.n, params.clone(), request).await
            }
            _ => Ran {
                reply: Err(Error::method_not_found()),
                end: done(),
            },
        }
    }

    fn denied(&mut self, error: Error) -> Ran {
        if let Some(live) = self.live.as_mut() {
            live.gate.breaker().denied();
        }
        Ran {
            reply: Err(error),
            end: StepEnd::Refused(CallRefusal::Denied(DenyCode::NotAllowed)),
        }
    }

    fn read(&mut self, request: &ReadTextFileRequest) -> Ran {
        let Some(live) = self.live.as_mut() else {
            return self.denied(fault::unknown_session());
        };
        if live.agent.as_ref() != Some(&request.session_id) {
            return self.denied(fault::unknown_session());
        }
        if live.gate.breaker().trip().is_some() {
            return self.denied(fault::not_now("paused"));
        }
        let confined = request
            .path
            .to_str()
            .and_then(|t| named(&live.cwd, t).ok())
            .and_then(|path| {
                let real = self.files.real(&path).ok()?;
                confine(&live.real_cwd, path, real).ok()
            });
        let Some(confined) = confined else {
            return self.denied(fault::invalid("not a path this session may read"));
        };
        match self.files.read(&confined.real, &live.real_cwd) {
            Ok(content) => {
                // What the file held is untrusted text now in the agent's hands: the session is
                // tainted, and the record says a read happened, not what it read.
                live.gate.taint();
                live.gate.note(Audit::Allowed {
                    action: "read".to_owned(),
                    basis: Basis::Unasked,
                });
                let text = lines_of(&content, request.line, request.limit);
                Ran {
                    reply: ok(&ReadTextFileResponse::new(text)),
                    end: done(),
                }
            }
            Err(why) => Ran {
                reply: Err(fault::invalid("the file could not be read")),
                end: file_end(why),
            },
        }
    }

    async fn write(&mut self, n: u64, request: &WriteTextFileRequest) -> Ran {
        let now = self.ticks.now();
        let Some(live) = self.live.as_mut() else {
            return self.denied(fault::unknown_session());
        };
        if live.agent.as_ref() != Some(&request.session_id) {
            return self.denied(fault::unknown_session());
        }
        let confined = request
            .path
            .to_str()
            .and_then(|t| named(&live.cwd, t).ok())
            .and_then(|path| {
                let real = self.files.real(&path).ok()?;
                confine(&live.real_cwd, path, real).ok()
            });
        let Some(confined) = confined else {
            return self.denied(fault::invalid("not a path this session may write"));
        };
        let ruling = live.gate.write(now, n, &confined).await;
        let Ruling::Allow(_) = ruling else {
            let Ruling::Refuse(why) = ruling else {
                unreachable!()
            };
            let trip = live.gate.breaker().trip();
            return Ran {
                reply: Err(fault::not_now("the write was not allowed")),
                end: refused_end(why, trip),
            };
        };
        match self
            .files
            .write(&confined.real, &live.real_cwd, &request.content)
        {
            Ok(before) => {
                self.undo.push(UndoNote {
                    path: confined.real,
                    before,
                });
                if self.undo.len() > UNDO_KEEP {
                    self.undo.remove(0);
                }
                Ran {
                    reply: ok(&WriteTextFileResponse::new()),
                    end: done(),
                }
            }
            Err(why) => Ran {
                reply: Err(fault::invalid("the file could not be written")),
                end: file_end(why),
            },
        }
    }

    async fn permission(&mut self, n: u64, request: &RequestPermissionRequest) -> Ran {
        let now = self.ticks.now();
        let program = self.program.clone();
        let Some(live) = self.live.as_mut() else {
            return self.denied(fault::unknown_session());
        };
        if live.agent.as_ref() != Some(&request.session_id) {
            return self.denied(fault::unknown_session());
        }
        let Some(tool) = tool_req(
            &program,
            &live.cwd,
            &live.real_cwd,
            &self.files,
            &request.tool_call,
        ) else {
            return self.denied(fault::invalid("not a tool call"));
        };
        let ruling = live.gate.tool(now, n, &tool).await;
        let pick = |kind: PermissionOptionKind| {
            request
                .options
                .iter()
                .find(|o| o.kind == kind)
                .map(|o| o.option_id.clone())
        };
        // Our answer is always the "once" option: an "always" the agent offered is never chosen,
        // so its own memory of one never exists; ours is the standing grant in our store. With no
        // matching "once" option the answer is cancelled.
        let outcome = match &ruling {
            Ruling::Allow(_) => pick(PermissionOptionKind::AllowOnce),
            Ruling::Refuse(_) => pick(PermissionOptionKind::RejectOnce),
        }
        .map_or(RequestPermissionOutcome::Cancelled, |option| {
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option))
        });
        Ran {
            reply: ok(&RequestPermissionResponse::new(outcome)),
            end: done(),
        }
    }

    async fn create(
        &mut self,
        n: u64,
        params: Value,
        request: &agent_client_protocol_schema::v1::CreateTerminalRequest,
    ) -> Ran {
        let now = self.ticks.now();
        let Some(live) = self.live.as_mut() else {
            return self.denied(fault::unknown_session());
        };
        if live.agent.as_ref() != Some(&request.session_id) {
            return self.denied(fault::unknown_session());
        }
        if live.gate.breaker().trip().is_some() {
            return self.denied(fault::not_now("paused"));
        }
        let line = docket_shell::Argv::new(&request.command, &request.args).map(|a| a.line());
        if let Some(line) = &line
            && live.gate.spend_command(line)
        {
            live.terminals.approve_once(line);
        }
        live.epoch.set(n);
        live.terminals.set_posture(live.gate.posture());
        live.terminals.load_grants(live.gate.grants().to_vec());
        let method = agent_client_protocol_schema::v1::CLIENT_METHOD_NAMES.terminal_create;
        let answer = live
            .terminals
            .handle(now, method, params)
            .await
            .unwrap_or_else(|| Err(Error::method_not_found()));
        live.gate.set_grants(live.terminals.grants().to_vec());
        let end = match &answer {
            Ok(_) => {
                live.gate.breaker().allowed();
                done()
            }
            Err(_) => {
                live.gate.breaker().denied();
                StepEnd::Refused(CallRefusal::Denied(DenyCode::NotAllowed))
            }
        };
        Ran { reply: answer, end }
    }
}
