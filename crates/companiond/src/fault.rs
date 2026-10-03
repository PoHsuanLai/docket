//! Why a companion entry point did not do what was asked.

use docket_client::ClientError;

/// Why the daemon could not serve, or an entry point could not answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServeFault {
    /// The bus name is taken or the connection failed.
    #[error("bus: {0}")]
    Bus(String),
    /// The router did not answer, or refused the request.
    #[error("the router: {0}")]
    Router(ClientError),
    /// No task of this companion runs on that session.
    #[error("no such session")]
    UnknownSession,
    /// The turn was not heard: companiond learns what the person said from `heard`.
    #[error("no such turn")]
    UnknownTurn,
    /// The task is over: a follow-up opens a new session.
    #[error("the task is finished")]
    Finished,
    /// An id the companion minted is not one.
    #[error("malformed id")]
    Malformed,
}
