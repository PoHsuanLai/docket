//! The shell tool: a table of terminals over a `Sandbox`. It is the one place a command is
//! started, and it starts nothing the sandbox cannot confine: if the sandbox cannot, `create`
//! refuses with the reason and runs nothing.
//!
//! `output` is bounded and redacted; `view` turns a long output into a `Handle` for a reader that
//! must not see untrusted text (the planner). `kill` and `release` always work: no gate stands
//! in front of stopping a command.

use crate::env::sandbox_env;
use crate::output::{Shown, shown};
use crate::sandbox::{Argv, ByteLimit, ExitReport, Job, Network, RunSpec, Sandbox, StartFault};
use docket_core::{AbsPath, CannotSandbox, Handle, SandboxState};
use std::collections::BTreeMap;

/// Output kept when the requester names no limit.
pub const DEFAULT_KEEP: usize = 64 * 1024;

/// The most output ever kept, whatever the requester asks.
pub const MAX_KEEP: usize = 1024 * 1024;

/// Output longer than this reaches a planner as a handle.
pub const INLINE_MAX: usize = 2048;

/// Terminals open at once.
pub const MAX_TERMINALS: usize = 8;

/// One terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TermId(pub u64);

/// A request to run one command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// The command.
    pub argv: Argv,
    /// Where it runs, and the one place it may write.
    pub cwd: AbsPath,
    /// The environment the requester asked for; only the allowlist survives.
    pub env: Vec<(String, String)>,
    /// Output bytes to keep, if the requester named a limit.
    pub limit: Option<u64>,
}

/// Why a terminal call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShellFault {
    /// The command cannot be sandboxed; nothing ran.
    #[error("cannot sandbox: {0}")]
    CannotSandbox(CannotSandbox),
    /// The sandbox program did not start.
    #[error("the sandbox program did not start")]
    Spawn,
    /// No such terminal (never made, or released).
    #[error("no such terminal")]
    NoSuchTerminal,
    /// Too many terminals are open.
    #[error("too many terminals are open")]
    TooMany,
}

impl From<StartFault> for ShellFault {
    fn from(fault: StartFault) -> Self {
        match fault {
            StartFault::Cannot(why) => ShellFault::CannotSandbox(why),
            StartFault::Spawn => ShellFault::Spawn,
        }
    }
}

/// A terminal's output and how it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The output, redacted and within the limit.
    pub shown: Shown,
    /// How it ended, if it has.
    pub exit: Option<ExitReport>,
}

/// Output as a planner sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    /// Short enough to show.
    Inline(Snapshot),
    /// Too long: held behind a handle.
    Held {
        /// The handle.
        handle: Handle,
        /// Its length in bytes.
        bytes: usize,
        /// How it ended, if it has.
        exit: Option<ExitReport>,
    },
}

#[derive(Debug)]
struct Term<J> {
    job: J,
    limit: ByteLimit,
}

/// The terminals of one connection.
#[derive(Debug)]
pub struct Shell<S: Sandbox> {
    sandbox: S,
    network: Network,
    terms: BTreeMap<TermId, Term<S::Job>>,
    next: u64,
    held: BTreeMap<Handle, String>,
}

fn limit_of(asked: Option<u64>) -> ByteLimit {
    let asked = asked.map_or(DEFAULT_KEEP, |n| usize::try_from(n).unwrap_or(MAX_KEEP));
    ByteLimit(asked.clamp(1, MAX_KEEP))
}

impl<S: Sandbox> Shell<S> {
    /// A shell over `sandbox`, whose commands have no network.
    pub fn new(sandbox: S) -> Self {
        Self::with_network(sandbox, Network::Off)
    }

    /// A shell over `sandbox` whose commands run with `network`.
    pub fn with_network(sandbox: S, network: Network) -> Self {
        Self {
            sandbox,
            network,
            terms: BTreeMap::new(),
            next: 0,
            held: BTreeMap::new(),
        }
    }

    /// The network the commands run with.
    pub fn network(&self) -> Network {
        self.network
    }

    /// Whether the sandbox can confine anything at all.
    pub fn available(&self) -> SandboxState {
        self.sandbox.available()
    }

    /// Whether a command in `cwd` can be sandboxed.
    pub fn check(&self, cwd: &AbsPath) -> SandboxState {
        self.sandbox.check(cwd)
    }

    /// Starts a command in the sandbox and returns its terminal.
    pub fn create(&mut self, launch: &Launch) -> Result<TermId, ShellFault> {
        if self.terms.len() >= MAX_TERMINALS {
            return Err(ShellFault::TooMany);
        }
        let limit = limit_of(launch.limit);
        let spec = RunSpec {
            argv: launch.argv.clone(),
            cwd: launch.cwd.clone(),
            env: sandbox_env(&launch.env),
            network: self.network,
            keep: limit,
        };
        let job = self.sandbox.start(&spec)?;
        self.next += 1;
        let id = TermId(self.next);
        self.terms.insert(id, Term { job, limit });
        Ok(id)
    }

    /// The output so far, and the exit if there is one.
    pub fn output(&mut self, id: TermId) -> Result<Snapshot, ShellFault> {
        let term = self.terms.get_mut(&id).ok_or(ShellFault::NoSuchTerminal)?;
        let exit = term.job.exit();
        Ok(Snapshot {
            shown: shown(&term.job.output(), term.limit),
            exit,
        })
    }

    /// Blocks until the command ends.
    pub fn wait(&mut self, id: TermId) -> Result<ExitReport, ShellFault> {
        let term = self.terms.get_mut(&id).ok_or(ShellFault::NoSuchTerminal)?;
        Ok(term.job.wait())
    }

    /// Kills the command; the terminal stays valid, so its output can still be read.
    pub fn kill(&mut self, id: TermId) -> Result<(), ShellFault> {
        let term = self.terms.get_mut(&id).ok_or(ShellFault::NoSuchTerminal)?;
        term.job.kill();
        Ok(())
    }

    /// Kills the command and forgets the terminal.
    pub fn release(&mut self, id: TermId) -> Result<(), ShellFault> {
        let mut term = self.terms.remove(&id).ok_or(ShellFault::NoSuchTerminal)?;
        term.job.kill();
        Ok(())
    }

    /// The output as a planner may see it: a long one is held behind a handle.
    pub fn view(&mut self, id: TermId) -> Result<View, ShellFault> {
        let snap = self.output(id)?;
        if snap.shown.text.len() <= INLINE_MAX {
            return Ok(View::Inline(snap));
        }
        let handle = Handle(self.held.len() as u64 + 1);
        let bytes = snap.shown.text.len();
        self.held.insert(handle, snap.shown.text);
        Ok(View::Held {
            handle,
            bytes,
            exit: snap.exit,
        })
    }

    /// What a handle holds: for the person's own view, never a planner's.
    pub fn held(&self, handle: Handle) -> Option<&str> {
        self.held.get(&handle).map(String::as_str)
    }

    /// Terminals open now.
    pub fn open(&self) -> usize {
        self.terms.len()
    }
}
