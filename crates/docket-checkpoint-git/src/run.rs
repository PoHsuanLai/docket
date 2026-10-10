//! The one effect: running git as a process. No shell is involved: the arguments are a list.

use docket_core::AbsPath;
use std::process::{ExitStatus, Stdio};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// Variables that would point git at another repository, index or object store than the one
/// the command names. They are never inherited: a command sets exactly the ones it needs.
const REDIRECTING: [&str; 12] = [
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
];

/// Why git could not be run at all (a command that ran and failed is a [`GitOut`]).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum GitFault {
    /// The process did not start: no git, or no such folder.
    #[error("git could not be started")]
    Spawn,
    /// Its pipes broke or it could not be waited for.
    #[error("git could not be run to the end")]
    Io,
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
}

/// What a command that ran left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitOut {
    /// How it ended.
    pub status: ExitStatus,
    /// What it printed.
    pub stdout: Vec<u8>,
}

impl GitOut {
    /// Whether it ended with success.
    pub fn succeeded(&self) -> bool {
        self.status.success()
    }
}

/// The seam: runs a command and gives back what it printed.
pub trait GitRun: Send + Sync {
    /// Runs `cmd` to its end.
    fn run(
        &self,
        cmd: GitCommand,
    ) -> impl std::future::Future<Output = Result<GitOut, GitFault>> + Send;
}

/// Runs the `git` found on `PATH` as a child process; a command dropped before it ends is killed.
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
            .stderr(Stdio::null())
            .kill_on_drop(true);
        for name in REDIRECTING {
            process.env_remove(name);
        }
        process.env("GIT_TERMINAL_PROMPT", "0");
        process.envs(cmd.env.iter().map(|(k, v)| (k, v)));
        let mut child = process.spawn().map_err(|_| GitFault::Spawn)?;
        let pipe = child.stdin.take();
        let input = cmd.stdin;
        let feed = async move {
            if let (Some(mut pipe), Some(bytes)) = (pipe, input) {
                // A command that stops reading early closes the pipe; its exit status says why.
                let _ = pipe.write_all(&bytes).await;
                let _ = pipe.shutdown().await;
            }
        };
        let (_, done) = tokio::join!(feed, child.wait_with_output());
        let done = done.map_err(|_| GitFault::Io)?;
        Ok(GitOut {
            status: done.status,
            stdout: done.stdout,
        })
    }
}
