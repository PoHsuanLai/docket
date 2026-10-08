//! The five `terminal/*` methods, served over the sandboxed shell tool. These are client methods:
//! the party that runs commands answers them. When an external agent drives us (S4) it sends them
//! to us; here they run in our sandbox, behind our gate, never in the editor's own terminal.
//!
//! `terminal/create` is the `Execute` effect, which the router rules (`acpagent.terminal.run`);
//! this only runs what the router allowed. A command that is not sandboxable is refused before
//! the router is asked (we never run unsandboxed, so there is nothing to approve).
//! `terminal/kill` and `terminal/release` are always allowed.
//!
//! `terminal/wait_for_exit` blocks the thread until the command ends; a host that shares a thread
//! with other work uses `poll_wait`.

use crate::fault;
use agent_client_protocol_schema::v1::{
    CLIENT_METHOD_NAMES, ClientCapabilities, CreateTerminalRequest, CreateTerminalResponse, Error,
    KillTerminalRequest, KillTerminalResponse, ReleaseTerminalRequest, ReleaseTerminalResponse,
    TerminalExitStatus, TerminalId, TerminalOutputRequest, TerminalOutputResponse,
    WaitForTerminalExitRequest, WaitForTerminalExitResponse,
};
use docket_core::{AbsPath, Cover, SandboxState};
use docket_shell::{Argv, Cut, ExitReport, Launch, Sandbox, Shell, ShellFault, TermId};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// The terminal methods for one connection.
#[derive(Debug)]
pub struct Terminals<S: Sandbox> {
    shell: Shell<S>,
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

impl<S: Sandbox> Terminals<S> {
    /// The terminal methods over `sandbox`.
    pub fn new(sandbox: S) -> Self {
        Self {
            shell: Shell::new(sandbox),
            owners: BTreeMap::new(),
        }
    }

    /// `base` with the terminal capability on exactly when the sandbox is there. With none, the
    /// capability stays off and an agent is told it has no terminal.
    pub fn capabilities(&self, base: ClientCapabilities) -> ClientCapabilities {
        base.terminal(self.shell.available() == SandboxState::Ready)
    }

    /// Whether `cwd` can be confined, and if not why not: a command that cannot be sandboxed is
    /// refused before anyone is asked.
    pub fn check(&self, cwd: &AbsPath) -> SandboxState {
        self.shell.check(cwd)
    }

    /// Answers one `terminal/*` request other than `create` (which needs the session's scope and
    /// the router's yes: `create`); `None` for any other method.
    pub fn handle(&mut self, method: &str, params: Value) -> Option<Result<Value, Error>> {
        let names = CLIENT_METHOD_NAMES;
        let answer = match method {
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

    /// Starts the command a `terminal/create` asked for, in `scope` or below it. The router has
    /// ruled on it; this only refuses what cannot be confined or runs outside the session.
    pub fn create(&mut self, scope: &AbsPath, params: Value) -> Result<Value, Error> {
        let asked: CreateTerminalRequest = fault::params(params)?;
        let cwd = match &asked.cwd {
            Some(path) => AbsPath::parse(&path.to_string_lossy())
                .map_err(|_| fault::invalid("the working directory must be absolute"))?,
            None => scope.clone(),
        };
        if scope.covers(&cwd) != Cover::Covers {
            return Err(fault::invalid(
                "the working directory is outside the session",
            ));
        }
        let argv = Argv::new(&asked.command, &asked.args)
            .ok_or_else(|| fault::invalid("the command is empty or holds a NUL"))?;
        // We never run unsandboxed: a command that cannot be confined is refused here, with the
        // reason.
        if let SandboxState::Cannot(why) = self.shell.check(&cwd) {
            return Err(shell_error(&ShellFault::CannotSandbox(why)));
        }
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

    /// `terminal/wait_for_exit` without blocking: the answer if the command has ended, `None`
    /// while it runs. A host that cannot block polls this.
    pub fn poll_wait(&mut self, params: Value) -> Result<Option<Value>, Error> {
        let asked: WaitForTerminalExitRequest = fault::params(params)?;
        let id = self.term(&asked.session_id.0, &asked.terminal_id)?;
        let snap = self.shell.output(id).map_err(|f| shell_error(&f))?;
        snap.exit
            .map(|report| out(&WaitForTerminalExitResponse::new(exit_status(report))))
            .transpose()
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
