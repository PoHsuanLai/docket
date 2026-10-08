//! The five `terminal/*` methods, served over the sandboxed shell tool. These are client methods:
//! the party that runs commands answers them. When an external agent drives us (S4) it sends them
//! to us; here they run in our sandbox, behind our gate, never in the editor's own terminal.
//!
//! `terminal/create` is the `Execute` effect: `rule_execute` rules it. A command that is not
//! sandboxable is refused before the person is asked (we never run unsandboxed, so there is
//! nothing to approve). `terminal/kill` and `terminal/release` are always allowed.
//!
//! `terminal/wait_for_exit` blocks the thread until the command ends; a host that shares a thread
//! with other work calls `handle` from a blocking task.

use crate::fault;
use crate::terminal_ask::{Answer, Decide, Note, Posture, TerminalAsk};
use agent_client_protocol_schema::v1::{
    CLIENT_METHOD_NAMES, ClientCapabilities, CreateTerminalRequest, CreateTerminalResponse, Error,
    KillTerminalRequest, KillTerminalResponse, ReleaseTerminalRequest, ReleaseTerminalResponse,
    TerminalExitStatus, TerminalId, TerminalOutputRequest, TerminalOutputResponse,
    WaitForTerminalExitRequest, WaitForTerminalExitResponse,
};
use docket_core::{
    AbsPath, ArgFacts, CallFacts, Cover, ExecuteFacts, ExecuteRuling, GrantCaller, SandboxState,
    StandingGrant, held_with, rule_execute,
};
use docket_shell::{Argv, Cut, ExitReport, Launch, Sandbox, Shell, ShellFault, TermId};
use prov::UnixSeconds;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// The terminal methods for one connection and one caller.
#[derive(Debug)]
pub struct Terminals<S: Sandbox, D> {
    shell: Shell<S>,
    decide: D,
    caller: GrantCaller,
    action: docket_core::ActionRef,
    scope: AbsPath,
    grants: Vec<StandingGrant>,
    posture: Posture,
    notes: Vec<Note>,
    owners: BTreeMap<TermId, String>,
}

fn out(value: &impl Serialize) -> Result<Value, Error> {
    serde_json::to_value(value).map_err(|_| fault::internal("could not answer"))
}

fn exit_status(report: ExitReport) -> TerminalExitStatus {
    match report {
        ExitReport::Code(code) => TerminalExitStatus::new().exit_code(code),
        ExitReport::Signal(name) => TerminalExitStatus::new().signal(name),
    }
}

fn shell_error(fault: &ShellFault) -> Error {
    match fault {
        ShellFault::NoSuchTerminal => fault::invalid("no such terminal"),
        ShellFault::CannotSandbox(why) => fault::not_now(&format!("cannot sandbox: {why}")),
        ShellFault::TooMany => fault::not_now("too many terminals are open"),
        ShellFault::Spawn => fault::internal("the sandbox did not start"),
    }
}

impl<S: Sandbox, D: Decide> Terminals<S, D> {
    /// The terminal methods for `caller`, whose commands may run at or below `scope` (the
    /// session's working directory). `action` names the `Execute` action the gate rules on.
    pub fn new(
        sandbox: S,
        decide: D,
        caller: GrantCaller,
        action: docket_core::ActionRef,
        scope: AbsPath,
    ) -> Self {
        Self {
            shell: Shell::new(sandbox),
            decide,
            caller,
            action,
            scope,
            grants: Vec::new(),
            posture: Posture::default(),
            notes: Vec::new(),
            owners: BTreeMap::new(),
        }
    }

    /// `base` with the terminal capability on exactly when the sandbox is there. With none, the
    /// capability stays off and an agent is told it has no terminal.
    pub fn capabilities(&self, base: ClientCapabilities) -> ClientCapabilities {
        base.terminal(self.shell.available() == SandboxState::Ready)
    }

    /// Sets what the host knows about the session before the next command.
    pub fn set_posture(&mut self, posture: Posture) {
        self.posture = posture;
    }

    /// The standing grants held (the store the person revokes from).
    pub fn grants(&self) -> &[StandingGrant] {
        &self.grants
    }

    /// Loads the grants the store holds.
    pub fn load_grants(&mut self, grants: Vec<StandingGrant>) {
        self.grants = grants;
    }

    /// The audit notes since the last call.
    pub fn take_notes(&mut self) -> Vec<Note> {
        std::mem::take(&mut self.notes)
    }

    /// Answers one `terminal/*` request; `None` for any other method.
    pub async fn handle(
        &mut self,
        now: UnixSeconds,
        method: &str,
        params: Value,
    ) -> Option<Result<Value, Error>> {
        let names = CLIENT_METHOD_NAMES;
        let answer = match method {
            m if m == names.terminal_create => self.create(now, params).await,
            m if m == names.terminal_output => self.output(params),
            m if m == names.terminal_wait_for_exit => self.wait(params),
            m if m == names.terminal_kill => self.kill(params),
            m if m == names.terminal_release => self.release(params),
            _ => return None,
        };
        Some(answer)
    }

    fn term(&self, session: &str, id: &TerminalId) -> Result<TermId, Error> {
        let n =
            id.0.strip_prefix("term-")
                .and_then(|n| n.parse::<u64>().ok())
                .map(TermId)
                .ok_or_else(|| fault::invalid("no such terminal"))?;
        match self.owners.get(&n) {
            Some(owner) if owner == session => Ok(n),
            _ => Err(fault::invalid("no such terminal")),
        }
    }

    async fn create(&mut self, now: UnixSeconds, params: Value) -> Result<Value, Error> {
        let asked: CreateTerminalRequest = fault::params(params)?;
        let cwd = match &asked.cwd {
            Some(path) => AbsPath::parse(&path.to_string_lossy())
                .map_err(|_| fault::invalid("the working directory must be absolute"))?,
            None => self.scope.clone(),
        };
        if self.scope.covers(&cwd) != Cover::Covers {
            return Err(fault::invalid(
                "the working directory is outside the session",
            ));
        }
        let argv = Argv::new(&asked.command, &asked.args)
            .ok_or_else(|| fault::invalid("the command is empty or holds a NUL"))?;
        let line = argv.line();
        // We never run unsandboxed: a command that cannot be confined is refused here, with the
        // reason, and the person is not asked to approve what cannot run.
        if let SandboxState::Cannot(why) = self.shell.check(&cwd) {
            self.notes.push(Note::Refused { line });
            return Err(shell_error(&ShellFault::CannotSandbox(why)));
        }
        self.gate(now, &line, &cwd).await?;
        let launch = Launch {
            argv,
            cwd,
            env: asked
                .env
                .iter()
                .map(|v| (v.name.clone(), v.value.clone()))
                .collect(),
            limit: asked.output_byte_limit,
        };
        let id = self.shell.create(&launch).map_err(|f| shell_error(&f))?;
        self.owners.insert(id, asked.session_id.0.to_string());
        out(&CreateTerminalResponse::new(TerminalId::new(format!(
            "term-{}",
            id.0
        ))))
    }

    /// Runs the `Execute` rules, asking the person where they say to.
    async fn gate(&mut self, now: UnixSeconds, line: &str, cwd: &AbsPath) -> Result<(), Error> {
        let call = CallFacts {
            action: self.action.clone(),
            args: ArgFacts::Command {
                line: line.to_owned(),
                cwd: cwd.clone(),
            },
        };
        let facts = ExecuteFacts {
            caller: &self.caller,
            call: &call,
            sandbox: self.shell.check(cwd),
            origin: self.posture.origin,
            breaker: self.posture.breaker,
            budget: self.posture.budget,
            review: self.posture.review,
        };
        let refused = |notes: &mut Vec<Note>| {
            notes.push(Note::Refused {
                line: line.to_owned(),
            });
            fault::not_now("the command was not allowed")
        };
        match rule_execute(&facts, &self.grants) {
            ExecuteRuling::Deny => Err(refused(&mut self.notes)),
            ExecuteRuling::Run(grant) => {
                self.notes.push(Note::GrantUsed {
                    grant,
                    line: line.to_owned(),
                });
                Ok(())
            }
            ExecuteRuling::Ask { why, offer } => {
                let ask = TerminalAsk {
                    line: line.to_owned(),
                    cwd: cwd.clone(),
                    why,
                    offer: offer.clone(),
                };
                match self.decide.decide(&ask).await {
                    Answer::No => Err(refused(&mut self.notes)),
                    answer => {
                        self.remember(now, answer, &offer);
                        self.notes.push(Note::Confirmed {
                            line: line.to_owned(),
                        });
                        Ok(())
                    }
                }
            }
        }
    }

    fn remember(&mut self, now: UnixSeconds, answer: Answer, offer: &docket_core::AlwaysOffer) {
        if let (Answer::Always, docket_core::AlwaysOffer::Offered(scope)) = (answer, offer) {
            let grant = StandingGrant::new(self.caller.clone(), scope.clone(), now);
            self.notes.push(Note::GrantStored {
                grant: grant.id.clone(),
            });
            self.grants = held_with(std::mem::take(&mut self.grants), grant);
        }
    }

    fn output(&mut self, params: Value) -> Result<Value, Error> {
        let asked: TerminalOutputRequest = fault::params(params)?;
        let id = self.term(&asked.session_id.0, &asked.terminal_id)?;
        let snap = self.shell.output(id).map_err(|f| shell_error(&f))?;
        let cut = snap.shown.cut == Cut::Head;
        out(&TerminalOutputResponse::new(snap.shown.text, cut)
            .exit_status(snap.exit.map(exit_status)))
    }

    fn wait(&mut self, params: Value) -> Result<Value, Error> {
        let asked: WaitForTerminalExitRequest = fault::params(params)?;
        let id = self.term(&asked.session_id.0, &asked.terminal_id)?;
        let report = self.shell.wait(id).map_err(|f| shell_error(&f))?;
        out(&WaitForTerminalExitResponse::new(exit_status(report)))
    }

    fn kill(&mut self, params: Value) -> Result<Value, Error> {
        let asked: KillTerminalRequest = fault::params(params)?;
        let id = self.term(&asked.session_id.0, &asked.terminal_id)?;
        self.shell.kill(id).map_err(|f| shell_error(&f))?;
        out(&KillTerminalResponse::new())
    }

    fn release(&mut self, params: Value) -> Result<Value, Error> {
        let asked: ReleaseTerminalRequest = fault::params(params)?;
        let id = self.term(&asked.session_id.0, &asked.terminal_id)?;
        self.shell.release(id).map_err(|f| shell_error(&f))?;
        self.owners.remove(&id);
        out(&ReleaseTerminalResponse::new())
    }
}
