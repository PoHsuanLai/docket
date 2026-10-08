//! The agent's own login, run where the person can see it. accountd asks; we run the command the
//! entry names for that program (a terminal emulator running the agent's `login`, say), outside
//! the agent sandbox, as the person's own program, with stdio nowhere: docket reads no output, no
//! URL, no code and no token. The report is a coarse outcome from a closed set, and `Ready`
//! means only that the command ended well; whether the agent is signed in is the agent's word,
//! found when a session starts.

use crate::accounts::AskKind;
use crate::config::Entry;
use porter_core::{LoginFault, LoginOutcome};
use std::future::Future;
use std::process::Stdio;
use tokio::process::Command;

/// Runs a login or a logout.
pub trait LoginRunner: Send + Sync {
    /// Runs the entry's command for `kind` and says how it went.
    fn run(&self, entry: &Entry, kind: AskKind) -> impl Future<Output = LoginOutcome> + Send;
}

/// The real runner. `env` is what the command needs to show itself (the display, the runtime
/// directory, `HOME`, `PATH`), chosen by the daemon: this crate reads no environment.
#[derive(Debug, Clone, Default)]
pub struct VisibleLogin {
    env: Vec<(String, String)>,
}

impl VisibleLogin {
    /// A runner that gives its commands exactly `env`.
    pub fn new(env: Vec<(String, String)>) -> Self {
        Self { env }
    }
}

impl LoginRunner for VisibleLogin {
    async fn run(&self, entry: &Entry, kind: AskKind) -> LoginOutcome {
        let words = match kind {
            AskKind::Login => &entry.login,
            AskKind::Logout => &entry.logout,
        };
        let Some((program, args)) = words.split_first() else {
            return LoginOutcome::Failed(LoginFault::NotInstalled);
        };
        let status = Command::new(program)
            .args(args)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .status()
            .await;
        match status {
            Ok(done) if done.success() => LoginOutcome::Ready,
            Ok(_) => LoginOutcome::Failed(LoginFault::Refused),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                LoginOutcome::Failed(LoginFault::NotInstalled)
            }
            Err(_) => LoginOutcome::Failed(LoginFault::Other),
        }
    }
}
