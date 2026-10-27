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
use crate::workdir::Workdir;
use acp_core::command_deadline::CommandDeadline;
use acp_core::fault;
use agent_client_protocol::schema::v1::{
    ClientCapabilities, CreateTerminalRequest, CreateTerminalResponse, Error, KillTerminalRequest,
    KillTerminalResponse, ReleaseTerminalRequest, ReleaseTerminalResponse, TerminalExitStatus,
    TerminalId, TerminalOutputRequest, TerminalOutputResponse, WaitForTerminalExitRequest,
    WaitForTerminalExitResponse,
};
use bulkhead::{
    Argv, Cut, ExitReport, Launch, Network, Sandbox, Shell, ShellFault, StartFault, TermId,
};
use docket_core::{AbsPath, Cover, NetAccess, SandboxState};
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

fn exit_status(report: ExitReport) -> TerminalExitStatus {
    match report {
        ExitReport::Code(code) => TerminalExitStatus::new().exit_code(code),
        ExitReport::Signal(name) => TerminalExitStatus::new().signal(name),
    }
}

fn shell_error(fault: &ShellFault) -> Error {
    match fault {
        ShellFault::NoSuchTerminal => fault::invalid("no such terminal"),
        ShellFault::TooMany => fault::not_now("too many terminals are open"),
        ShellFault::Start(StartFault::Cannot(why)) => {
            fault::not_now(&format!("cannot sandbox: {why}"))
        }
        ShellFault::Start(StartFault::InheritedFds) => {
            fault::not_now("the sandbox refused to start: a file descriptor would leak into it")
        }
        ShellFault::Start(StartFault::FilterFile) => {
            fault::internal("the sandbox's syscall filter was not given")
        }
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
        bulk::state(self.shell.check(cwd))
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
    pub fn create(
        &mut self,
        scope: &AbsPath,
        asked: &CreateTerminalRequest,
    ) -> Result<CreateTerminalResponse, Error> {
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
        self.start(asked, &cwd)
    }

    /// Starts the command a `terminal/create` asked for in `workdir`, a directory already
    /// confined (links resolved, inside the session, not home): the request's own `cwd` is not
    /// used, and the sandbox is given the real path.
    pub fn create_in(
        &mut self,
        workdir: &Workdir,
        asked: &CreateTerminalRequest,
    ) -> Result<CreateTerminalResponse, Error> {
        self.start(asked, workdir.real())
    }

    fn start(
        &mut self,
        asked: &CreateTerminalRequest,
        cwd: &AbsPath,
    ) -> Result<CreateTerminalResponse, Error> {
        self.reap();
        let argv = Argv::new(&asked.command, &asked.args)
            .ok_or_else(|| fault::invalid("the command is empty or holds a NUL"))?;
        // We never run unsandboxed: a command that cannot be confined is refused here, with the
        // reason.
        let shell_cwd = cwd.clone();
        if let bulkhead::SandboxState::Cannot(why) = self.shell.check(&shell_cwd) {
            return Err(shell_error(&ShellFault::Start(StartFault::Cannot(why))));
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
        Ok(CreateTerminalResponse::new(TerminalId::new(format!(
            "term-{}",
            id.0
        ))))
    }

    /// `terminal/output`: what the command has written so far, and how it ended if it has.
    pub fn output(
        &mut self,
        asked: &TerminalOutputRequest,
    ) -> Result<TerminalOutputResponse, Error> {
        self.reap();
        let id = self.term(&asked.session_id.0, &asked.terminal_id)?;
        let snap = self.shell.output(id).map_err(|f| shell_error(&f))?;
        let cut = snap.shown.cut == Cut::Head;
        Ok(TerminalOutputResponse::new(snap.shown.text, cut)
            .exit_status(snap.exit.map(exit_status)))
    }

    /// `terminal/wait_for_exit`: blocks the thread until the command ends (or is killed at its
    /// deadline). A host that shares a thread with other work uses [`Terminals::poll_wait`].
    pub fn wait(
        &mut self,
        asked: &WaitForTerminalExitRequest,
    ) -> Result<WaitForTerminalExitResponse, Error> {
        let id = self.term(&asked.session_id.0, &asked.terminal_id)?;
        let report = self.wait_until_due(id).map_err(|f| shell_error(&f))?;
        Ok(WaitForTerminalExitResponse::new(exit_status(report)))
    }

    /// `terminal/wait_for_exit` without blocking: the answer if the command has ended, `None`
    /// while it runs. A host that cannot block polls this.
    pub fn poll_wait(
        &mut self,
        asked: &WaitForTerminalExitRequest,
    ) -> Result<Option<WaitForTerminalExitResponse>, Error> {
        self.reap();
        let id = self.term(&asked.session_id.0, &asked.terminal_id)?;
        let snap = self.shell.output(id).map_err(|f| shell_error(&f))?;
        Ok(snap
            .exit
            .map(|report| WaitForTerminalExitResponse::new(exit_status(report))))
    }

    /// `terminal/kill`: ends the command; the terminal stays readable.
    pub fn kill(&mut self, asked: &KillTerminalRequest) -> Result<KillTerminalResponse, Error> {
        self.reap();
        let id = self.term(&asked.session_id.0, &asked.terminal_id)?;
        self.shell.kill(id).map_err(|f| shell_error(&f))?;
        Ok(KillTerminalResponse::new())
    }

    /// `terminal/release`: forgets the terminal.
    pub fn release(
        &mut self,
        asked: &ReleaseTerminalRequest,
    ) -> Result<ReleaseTerminalResponse, Error> {
        self.reap();
        let id = self.term(&asked.session_id.0, &asked.terminal_id)?;
        self.shell.release(id).map_err(|f| shell_error(&f))?;
        self.owners.remove(&id);
        self.due.remove(&id);
        Ok(ReleaseTerminalResponse::new())
    }
}
