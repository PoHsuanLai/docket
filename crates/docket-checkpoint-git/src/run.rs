//! The one effect: running git as a process. No shell is involved: the arguments are a list.

use bulkhead::AbsPath;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

/// Variables that would point git at another repository, index or object store than the one
/// the command names. They are never inherited: a command sets exactly the ones it needs.
const REDIRECTING: [&str; 21] = [
    "GIT_DIR",
    "GIT_INDEX_FILE",
    "GIT_WORK_TREE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
    "GIT_QUARANTINE_PATH",
    "GIT_AUTHOR_DATE",
    "GIT_COMMITTER_DATE",
    "GIT_EXTERNAL_DIFF",
    "GIT_CONFIG",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
    "GIT_ATTR_SOURCE",
    "GIT_EXEC_PATH",
    "GIT_ASKPASS",
    "GIT_SSH_COMMAND",
    "GIT_PAGER",
    "GIT_EDITOR",
];

/// How much of a command's standard error is kept: its last bytes.
const STDERR_TAIL: usize = 4096;

/// Why git could not be run to its end (a command that ran and failed is a [`GitOut`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum GitFault {
    /// The process did not start: no git, or no such folder.
    #[error("git could not be started")]
    Spawn,
    /// Its pipes broke or it could not be waited for.
    #[error("git could not be run to the end")]
    Io,
    /// It was still running at its deadline and was killed.
    #[error("git ran past its deadline")]
    TimedOut,
    /// It printed more than the limit and was killed.
    #[error("git printed more than the limit")]
    TooMuchOutput,
}

/// What one command may spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The command is killed if it has not ended by then.
    pub deadline: Duration,
    /// The command is killed if it prints more bytes than this.
    pub stdout_max: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            deadline: Duration::from_secs(120),
            stdout_max: 64 * 1024 * 1024,
        }
    }
}

/// One git command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommand {
    /// The folder it runs in.
    pub cwd: AbsPath,
    /// The arguments after `git`.
    pub args: Vec<String>,
    /// Variables set for this command, on top of the environment minus the redirecting ones.
    pub env: Vec<(String, String)>,
    /// Bytes written to its standard input.
    pub stdin: Option<Vec<u8>>,
    /// The deadline and the output limit.
    pub limits: Limits,
}

/// What a command that ran left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitOut {
    /// How it ended.
    pub status: ExitStatus,
    /// What it printed.
    pub stdout: Vec<u8>,
    /// The last bytes it printed on standard error, for the operator's log only.
    pub stderr: Vec<u8>,
}

impl GitOut {
    /// Whether it ended with success.
    pub fn succeeded(&self) -> bool {
        self.status.success()
    }
}

/// The seam: runs a command and gives back what it printed.
pub trait GitRun: Send + Sync {
    /// Runs `cmd` to its end, or to its deadline.
    fn run(
        &self,
        cmd: GitCommand,
    ) -> impl std::future::Future<Output = Result<GitOut, GitFault>> + Send;
}

/// Reads `pipe` to its end; more than `max` bytes is `TooMuchOutput`, without reading the rest.
async fn read_bounded<R: AsyncRead + Unpin>(pipe: &mut R, max: usize) -> Result<Vec<u8>, GitFault> {
    let cap = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1);
    let mut out = Vec::new();
    pipe.take(cap)
        .read_to_end(&mut out)
        .await
        .map_err(|_| GitFault::Io)?;
    if out.len() > max {
        return Err(GitFault::TooMuchOutput);
    }
    Ok(out)
}

/// Reads `pipe` to its end and keeps the last [`STDERR_TAIL`] bytes.
async fn read_tail<R: AsyncRead + Unpin>(pipe: &mut R) -> Result<Vec<u8>, GitFault> {
    let (mut tail, mut chunk) = (Vec::new(), [0u8; 1024]);
    loop {
        let n = pipe.read(&mut chunk).await.map_err(|_| GitFault::Io)?;
        if n == 0 {
            return Ok(tail);
        }
        tail.extend_from_slice(&chunk[..n]);
        if tail.len() > STDERR_TAIL {
            let cut = tail.len() - STDERR_TAIL;
            tail.drain(..cut);
        }
    }
}

/// Runs the `git` found on `PATH` as a child process; a command dropped before it ends is killed,
/// and so is one that passes its deadline or its output limit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StdGitRun;

impl GitRun for StdGitRun {
    async fn run(&self, cmd: GitCommand) -> Result<GitOut, GitFault> {
        let mut process = Command::new("git");
        process
            .args(&cmd.args)
            .current_dir(cmd.cwd.as_str())
            .stdin(match cmd.stdin {
                Some(_) => Stdio::piped(),
                None => Stdio::null(),
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for name in REDIRECTING {
            process.env_remove(name);
        }
        process.env("GIT_TERMINAL_PROMPT", "0");
        process.envs(cmd.env.iter().map(|(k, v)| (k, v)));
        let mut child = process.spawn().map_err(|why| {
            tracing::warn!(kind = ?why.kind(), "git could not be started");
            GitFault::Spawn
        })?;
        let (Some(mut stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take())
        else {
            let _ = child.kill().await;
            tracing::warn!("git's pipes were not set up");
            return Err(GitFault::Io);
        };
        let pipe = child.stdin.take();
        let input = cmd.stdin;
        let feed = async move {
            if let (Some(mut pipe), Some(bytes)) = (pipe, input) {
                // A command that stops reading early closes the pipe; its exit status says why.
                let _ = pipe.write_all(&bytes).await;
                let _ = pipe.shutdown().await;
            }
            Ok::<(), GitFault>(())
        };
        let limits = cmd.limits;
        let work = async {
            let (_, out, err) = tokio::try_join!(
                feed,
                read_bounded(&mut stdout, limits.stdout_max),
                read_tail(&mut stderr)
            )?;
            let status = child.wait().await.map_err(|why| {
                tracing::warn!(kind = ?why.kind(), "git could not be waited for");
                GitFault::Io
            })?;
            Ok::<GitOut, GitFault>(GitOut {
                status,
                stdout: out,
                stderr: err,
            })
        };
        let done = tokio::time::timeout(limits.deadline, work)
            .await
            .unwrap_or(Err(GitFault::TimedOut));
        if done.is_err() {
            let _ = child.kill().await;
        }
        done
    }
}
