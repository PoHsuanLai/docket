//! The process seam: starts one confined agent process and hands back its stdio and a handle.
//! `BwrapProcs` is the real one (bubblewrap as a separate program, never linked); `fake::FakeProcs`
//! records the run it was asked for and returns an in-memory pipe.

use docket_acp::LineWire;
use docket_acp::Wire;
use docket_shell::{AgentRun, agent_bwrap_args, present_hidden};
use std::future::Future;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::BufReader;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

/// Why a process did not start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProcFault {
    /// The sandbox program is missing or the kernel refuses it: nothing runs unconfined.
    #[error("no sandbox: the agent is not started unconfined")]
    NoSandbox,
    /// The process did not start.
    #[error("the process did not start")]
    Spawn,
}

/// A running agent process.
pub trait Proc: Send {
    /// Ends it now. Safe to repeat.
    fn kill(&mut self);
}

/// Starts confined agent processes.
pub trait Procs: Send {
    /// The process's stdio.
    type Wire: Wire;
    /// The handle.
    type Proc: Proc + 'static;

    /// Starts `run`: exactly its environment, its arguments, confined as its binds and network
    /// say.
    fn start(
        &mut self,
        run: &AgentRun,
    ) -> impl Future<Output = Result<(Self::Wire, Self::Proc), ProcFault>> + Send;
}

/// Bubblewrap, found at `program` by the caller (after `Detected::probe` said it works).
#[derive(Debug, Clone)]
pub struct BwrapProcs {
    program: PathBuf,
}

impl BwrapProcs {
    /// Runs agents with the bubblewrap at `program`.
    pub fn new(program: PathBuf) -> Self {
        Self { program }
    }
}

/// A tokio child.
#[derive(Debug)]
pub struct ChildProc {
    child: Child,
}

impl Proc for ChildProc {
    fn kill(&mut self) {
        let _ = self.child.start_kill();
    }
}

/// The agent's stdio as lines.
pub type StdioWire = LineWire<BufReader<ChildStdout>, ChildStdin>;

impl Procs for BwrapProcs {
    type Wire = StdioWire;
    type Proc = ChildProc;

    async fn start(&mut self, run: &AgentRun) -> Result<(StdioWire, ChildProc), ProcFault> {
        if !self.program.is_file() {
            return Err(ProcFault::NoSandbox);
        }
        let mut command = Command::new(&self.program);
        command
            .args(agent_bwrap_args(run, &present_hidden()))
            // Exactly the environment built for the agent, and nothing of ours; bubblewrap
            // passes it on, and none of it is on a command line.
            .env_clear()
            .envs(run.env.iter().map(|v| (&v.name, &v.value)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Agent logs can hold anything; they go nowhere.
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|_| ProcFault::Spawn)?;
        let stdin = child.stdin.take().ok_or(ProcFault::Spawn)?;
        let stdout = child.stdout.take().ok_or(ProcFault::Spawn)?;
        Ok((
            LineWire::new(BufReader::new(stdout), stdin),
            ChildProc { child },
        ))
    }
}
