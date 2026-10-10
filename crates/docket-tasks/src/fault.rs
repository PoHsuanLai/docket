//! Why a companion entry point did not do what was asked.

use docket_client::ClientError;

/// Why the daemon could not serve, or an entry point could not answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ServeFault {
    /// The bus name is taken or the connection failed.
    #[error("bus: {0}")]
    Bus(String),
    /// `companiond.toml` is not a configuration.
    #[error("{0}")]
    Config(String),
    /// The router did not answer, or refused the request.
    #[error("the router: {0}")]
    Router(ClientError),
    /// No task of this companion runs on that session.
    #[error("no such session")]
    UnknownSession,
    /// The task is over: a follow-up opens a new session.
    #[error("the task is finished")]
    Finished,
    /// The answer offers no card by that id.
    #[error("the answer has no such card")]
    NoSuchCard,
    /// An id the companion minted is not one.
    #[error("malformed id")]
    Malformed,
}
