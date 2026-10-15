//! What went wrong in a git command, kept for the operator's log. Never shown to a person: the
//! words on a sheet come from `StoreFault`, and git's own stderr can name paths and secrets.

use crate::run::GitFault;
use std::fmt;
use std::sync::Arc;

/// Why a git command did not give what was wanted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum GitCause {
    /// The process did not start.
    #[error("git could not be started")]
    Spawn,
    /// Its pipes broke or it could not be waited for.
    #[error("git could not be run to the end")]
    Io,
    /// It passed its deadline and was killed.
    #[error("git ran past its deadline")]
    TimedOut,
    /// It printed more than the limit and was killed.
    #[error("git printed more than the limit")]
    TooMuchOutput,
    /// It ended without success.
    #[error("git ended with status {code:?}: {stderr}")]
    Exit {
        /// The exit code, none if a signal ended it.
        code: Option<i32>,
        /// The last bytes of its standard error, as text.
        stderr: String,
    },
}

impl From<GitFault> for GitCause {
    fn from(fault: GitFault) -> Self {
        match fault {
            GitFault::Spawn => Self::Spawn,
            GitFault::TimedOut => Self::TimedOut,
            GitFault::TooMuchOutput => Self::TooMuchOutput,
            GitFault::Io => Self::Io,
        }
    }
}

/// A failed command: which git subcommand, and why.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("git {command}: {cause}")]
pub struct GitProblem {
    /// The subcommand (`add`, `update-ref`), without its arguments.
    pub command: String,
    /// Why it failed.
    #[source]
    pub cause: GitCause,
}

/// A function the daemon supplies to hear of problems.
type Sink = Arc<dyn Fn(&GitProblem) + Send + Sync>;

/// Where problems go: a function the daemon supplies, or nowhere.
#[derive(Clone, Default)]
pub(crate) struct Report(Option<Sink>);

impl Report {
    pub(crate) fn to(sink: impl Fn(&GitProblem) + Send + Sync + 'static) -> Self {
        Self(Some(Arc::new(sink)))
    }

    pub(crate) fn tell(&self, problem: &GitProblem) {
        if let Some(sink) = &self.0 {
            sink(problem);
        }
    }
}

impl fmt::Debug for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_some() {
            "Report(set)"
        } else {
            "Report(none)"
        })
    }
}
