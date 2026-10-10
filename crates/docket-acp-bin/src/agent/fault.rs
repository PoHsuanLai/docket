//! Why `docket-agent` stopped.

/// Why the agent host could not start or went on no longer. The words are the ones the program
/// prints, after `docket-agent: `.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AgentFault {
    /// The settings do not let an agent run.
    #[error("{0}")]
    Permit(String),
    /// `agents.toml` is not under the configuration directory.
    #[error("no agents.toml under the configuration directory")]
    NoAgentsFile,
    /// `agents.toml` is not a configuration.
    #[error("{0}")]
    Agents(String),
    /// No sandbox: no agent is started unconfined.
    #[error("no sandbox (bubblewrap) here: no agent is started unconfined")]
    NoSandbox,
    /// The network forwarder is not beside this program.
    #[error("cannot find docket-net-forward beside this program")]
    NoForwarder,
    /// The runtime directory is not named.
    #[error("no XDG_RUNTIME_DIR")]
    NoRuntimeDir,
    /// The session bus, or the launcher on it, did not answer.
    #[error("{0}")]
    Bus(String),
    /// The working directory cannot be used.
    #[error("{0}")]
    Directory(String),
    /// The working directory is not text.
    #[error("the directory is not UTF-8")]
    DirectoryNotUtf8,
    /// The session could not be opened.
    #[error("{0}")]
    Open(String),
    /// The session failed while it ran.
    #[error("{0}")]
    Session(String),
}
