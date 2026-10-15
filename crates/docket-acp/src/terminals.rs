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

use crate::bulk;
use crate::client::Workdir;
use crate::command_deadline::CommandDeadline;
use crate::fault;
use agent_client_protocol_schema::v1::{
    CLIENT_METHOD_NAMES, ClientCapabilities, CreateTerminalRequest, CreateTerminalResponse, Error,
    KillTerminalRequest, KillTerminalResponse, ReleaseTerminalRequest, ReleaseTerminalResponse,
    TerminalExitStatus, TerminalId, TerminalOutputRequest, TerminalOutputResponse,
    WaitForTerminalExitRequest, WaitForTerminalExitResponse,
};
use bulkhead::{Argv, Cut, ExitReport, Launch, Network, Sandbox, Shell, ShellFault, TermId};
use docket_core::{AbsPath, CannotSandbox, Cover, NetAccess, SandboxState};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// The terminal methods for one connection.
#[derive(Debug)]
pub struct Terminals<S: Sandbox> {
    shell: Shell<S>,
    owners: BTreeMap<TermId, String>,
    deadline: CommandDeadline,
    /// When each running command is killed. Looked at on every terminal call, since nothing else
    /// here runs between calls.
    due: BTreeMap<TermId, Instant>,
}

/// How often a wait for a command with a deadline looks at the clock.
const WAIT_STEP: Duration = Duration::from_millis(25);

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
        _ => fault::internal("the sandbox did not start"),
    }
}

impl<S: Sandbox> Terminals<S> {
    /// The terminal methods over `sandbox`.
    pub fn new(sandbox: S) -> Self {
        Self::with_network(sandbox, Network::Off)
    }

    /// The terminal methods over `sandbox`, whose commands run with `network`.
    pub fn with_network(sandbox: S, network: Network) -> Self {
        Self {
            shell: Shell::with_network(sandbox, network),
            owners: BTreeMap::new(),
            deadline: CommandDeadline::DEFAULT,
            due: BTreeMap::new(),
        }
    }

    /// Commands started from now on are killed `deadline` after they start.
    pub fn set_deadline(&mut self, deadline: CommandDeadline) {
        self.deadline = deadline;
    }

    /// Kills every command still running past its deadline.
    fn reap(&mut self) {
        let now = Instant::now();
        let late: Vec<TermId> = self
            .due
            .iter()
            .filter(|(_, due)| **due <= now)
            .map(|(id, _)| *id)
            .collect();
        for id in late {
            self.due.remove(&id);
            // A command that already ended is left alone; a kill that finds no terminal is a
            // terminal released meanwhile.
            if self.shell.output(id).is_ok_and(|snap| snap.exit.is_none()) {
                let _ = self.shell.kill(id);
            }
        }
    }

    /// The exit of `id`, waiting for it, but never past its deadline: then it is killed.
    fn wait_until_due(&mut self, id: TermId) -> Result<ExitReport, ShellFault> {
        loop {
            self.reap();
            if let Some(report) = self.shell.output(id)?.exit {
                return Ok(report);
            }
            let Some(due) = self.due.get(&id).copied() else {
                return self.shell.wait(id);
            };
            std::thread::sleep(due.saturating_duration_since(Instant::now()).min(WAIT_STEP));
        }
    }

    /// What network the commands have, for the router's rule on commands that can send data out.
    pub fn access(&self) -> NetAccess {
        bulk::access(self.shell.network())
    }

    /// `base` with the terminal capability on exactly when the sandbox is there. With none, the
    /// capability stays off and an agent is told it has no terminal.
    pub fn capabilities(&self, base: ClientCapabilities) -> ClientCapabilities {
        base.terminal(self.shell.available() == bulkhead::SandboxState::Ready)
    }

    /// Whether `cwd` can be confined, and if not why not: a command that cannot be sandboxed is
    /// refused before anyone is asked.
    pub fn check(&self, cwd: &AbsPath) -> SandboxState {
        bulk::to_shell(cwd).map_or(SandboxState::Cannot(CannotSandbox::BadWorkingDir), |path| {
            bulk::state(self.shell.check(&path))
        })
    }

    /// Answers one `terminal/*` request other than `create` (which needs the session's scope and
    /// the router's yes: `create`); `None` for any other method.
    pub fn handle(&mut self, method: &str, params: Value) -> Option<Result<Value, Error>> {
        self.reap();
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

    /// Starts the command a `terminal/create` asked for, in `scope` or below it, by the words
    /// of the request alone (no file system is asked: the directory is only compared by name).
    /// The router has ruled on it; this only refuses what cannot be confined or runs outside the
    /// session. The host that has a file system confines the directory first and calls
    /// [`Terminals::create_in`].
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
        self.start(&asked, &cwd)
    }

    /// Starts the command a `terminal/create` asked for in `workdir`, a directory already
    /// confined (links resolved, inside the session, not home): the request's own `cwd` is not
    /// used, and the sandbox is given the real path.
    pub fn create_in(&mut self, workdir: &Workdir, params: Value) -> Result<Value, Error> {
        let asked: CreateTerminalRequest = fault::params(params)?;
        self.start(&asked, workdir.real())
    }

    fn start(&mut self, asked: &CreateTerminalRequest, cwd: &AbsPath) -> Result<Value, Error> {
        self.reap();
        let argv = Argv::new(&asked.command, &asked.args)
            .ok_or_else(|| fault::invalid("the command is empty or holds a NUL"))?;
        // We never run unsandboxed: a command that cannot be confined is refused here, with the
        // reason.
        let shell_cwd = bulk::to_shell(cwd)
            .map_err(|_| fault::invalid("the working directory must be absolute"))?;
        if let bulkhead::SandboxState::Cannot(why) = self.shell.check(&shell_cwd) {
            return Err(shell_error(&ShellFault::CannotSandbox(why)));
        }
        let launch = Launch::new(argv, shell_cwd).with_env(
            asked
                .env
                .iter()
                .map(|v| (v.name.clone(), v.value.clone()))
                .collect(),
        );
        let launch = match asked.output_byte_limit {
            Some(bytes) => launch.with_limit(bytes),
            None => launch,
        };
        let id = self.shell.create(&launch).map_err(|f| shell_error(&f))?;
        self.owners.insert(id, asked.session_id.0.to_string());
        self.due.insert(id, Instant::now() + self.deadline.limit());
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
        let report = self.wait_until_due(id).map_err(|f| shell_error(&f))?;
        out(&WaitForTerminalExitResponse::new(exit_status(report)))
    }

    /// `terminal/wait_for_exit` without blocking: the answer if the command has ended, `None`
    /// while it runs. A host that cannot block polls this.
    pub fn poll_wait(&mut self, params: Value) -> Result<Option<Value>, Error> {
        self.reap();
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
        self.due.remove(&id);
        out(&ReleaseTerminalResponse::new())
    }
}
