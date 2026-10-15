//! The process seam: starts one confined agent process and hands back its stdio and a handle.
//! `BwrapProcs` is the real one (bubblewrap as a separate program, never linked); `fake::FakeProcs`
//! records the run it was asked for and returns an in-memory pipe.

use bulkhead::{AgentRun, BwrapSandbox, StartFault};
use docket_acp::LineWire;
use docket_acp::Wire;
use std::future::Future;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
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
    /// A file descriptor of ours would leak into the sandbox, or its syscall filter could not be
    /// handed over: nothing runs.
    #[error("the sandbox refused to start the agent")]
    Refused,
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

/// Bubblewrap with its hardening: resource limits and the syscall filter, as the detected
/// sandbox says. A sandbox that cannot apply them never reaches here (detection fails), and one
/// that cannot start the agent under them starts nothing.
#[derive(Debug, Clone)]
pub struct BwrapProcs {
    sandbox: BwrapSandbox,
    filter_dir: PathBuf,
}

/// Makes each filter file's name different within this process.
static FILTERS: AtomicU64 = AtomicU64::new(0);

impl BwrapProcs {
    /// Runs agents under `sandbox` (from `Detected::probe`). The syscall filter is written to a
    /// new private file in `filter_dir` for each start (a directory only the user can enter, such
    /// as `$XDG_RUNTIME_DIR`) and removed when the process ends.
    pub fn new(sandbox: BwrapSandbox, filter_dir: PathBuf) -> Self {
        Self {
            sandbox,
            filter_dir,
        }
    }

    /// A fresh, unused path for one start's filter file.
    fn filter_path(&self) -> Option<bulkhead::AbsPath> {
        let n = FILTERS.fetch_add(1, Ordering::Relaxed);
        let file = self
            .filter_dir
            .join(format!("docket-seccomp-{}-{n}.bpf", std::process::id()));
        bulkhead::AbsPath::parse(file.to_str()?).ok()
    }
}

/// A tokio child, and the filter file it was handed (removed when this goes).
#[derive(Debug)]
pub struct ChildProc {
    child: Child,
    filter: Option<PathBuf>,
}

impl Proc for ChildProc {
    fn kill(&mut self) {
        let _ = self.child.start_kill();
    }
}

impl Drop for ChildProc {
    fn drop(&mut self) {
        if let Some(file) = self.filter.take() {
            let _ = std::fs::remove_file(file);
        }
    }
}

/// The agent's stdio as lines.
pub type StdioWire = LineWire<BufReader<ChildStdout>, ChildStdin>;

fn refused(fault: StartFault) -> ProcFault {
    match fault {
        StartFault::Cannot(_) => ProcFault::NoSandbox,
        StartFault::InheritedFds | StartFault::FilterFile => ProcFault::Refused,
        _ => ProcFault::Spawn,
    }
}

impl Procs for BwrapProcs {
    type Wire = StdioWire;
    type Proc = ChildProc;

    async fn start(&mut self, run: &AgentRun) -> Result<(StdioWire, ChildProc), ProcFault> {
        let file = self.filter_path().ok_or(ProcFault::Spawn)?;
        // Held from here, so the file goes whichever way this returns.
        let mut filter = ChildFilter(Some(PathBuf::from(file.as_str())));
        self.sandbox
            .write_filter(&file)
            .map_err(|_| ProcFault::Spawn)?;
        // Limits, the syscall filter, exactly the environment of `run` (and none of ours, none of
        // it on a command line) all come with the command. It fails closed.
        let mut command = Command::from(
            self.sandbox
                .agent_command(run, Some(&file))
                .map_err(refused)?,
        );
        command
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
            ChildProc {
                child,
                filter: filter.0.take(),
            },
        ))
    }
}

/// A filter file that is removed unless it is handed on.
struct ChildFilter(Option<PathBuf>);

impl Drop for ChildFilter {
    fn drop(&mut self) {
        if let Some(file) = self.0.take() {
            let _ = std::fs::remove_file(file);
        }
    }
}
