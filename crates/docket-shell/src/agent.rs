//! The command line that confines a long-running agent process (acp-sessions.md R2): bubblewrap
//! as a separate program, built as plain arguments so a test reads them. Unlike a terminal
//! command, an agent talks over its stdio, so this file only builds the arguments; the process
//! and its pipes are the launcher's (`docket-launch`).
//!
//! What the agent gets: the host read-only; `/home`, `/root`, `/run`, `/tmp` and the like empty;
//! the session's working directory writable; each extra bind the person's config names for this
//! program (its own program directory read-only, its own login and state directory read-write);
//! the network per `NetworkMode`; a cleared environment that is exactly what the caller passes;
//! no capabilities; a new session; death with its parent.

use crate::bwrap::HIDDEN;
use crate::net::{AgentNet, EndpointBind};
use crate::sandbox::{Argv, EnvVar};
use docket_core::AbsPath;

/// Where the forwarder and its socket appear inside the sandbox.
pub const INSIDE_FORWARDER: &str = "/run/docket-net/forward";
/// The socket's path inside the sandbox.
pub const INSIDE_SOCKET: &str = "/run/docket-net/ep.sock";

/// How an extra path is bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Visible, not writable.
    ReadOnly,
    /// Visible and writable: only for the program's own state.
    ReadWrite,
}

/// One extra path made visible inside the sandbox at the same path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bind {
    /// The path, on the host and inside.
    pub path: AbsPath,
    /// How.
    pub access: Access,
}

/// Everything needed to confine one agent process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRun {
    /// The agent command.
    pub argv: Argv,
    /// The session's working directory, writable.
    pub cwd: AbsPath,
    /// The whole environment (already built); nothing else is inherited.
    pub env: Vec<EnvVar>,
    /// The network.
    pub net: AgentNet,
    /// The extra binds, in order.
    pub binds: Vec<Bind>,
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

/// The `bwrap` arguments for `run`, hiding the `hidden` directories. Pure.
pub fn agent_bwrap_args(run: &AgentRun, hidden: &[&str]) -> Vec<String> {
    let mut args = words(&["--die-with-parent", "--new-session", "--unshare-all"]);
    if run.net == AgentNet::Host {
        args.push("--share-net".to_owned());
    }
    args.extend(words(&[
        "--cap-drop",
        "ALL",
        "--ro-bind",
        "/",
        "/",
        "--dev",
        "/dev",
        "--proc",
        "/proc",
    ]));
    for dir in hidden {
        args.extend(["--tmpfs".to_owned(), (*dir).to_owned()]);
    }
    for bind in &run.binds {
        let flag = match bind.access {
            Access::ReadOnly => "--ro-bind",
            Access::ReadWrite => "--bind",
        };
        let path = bind.path.as_str();
        args.extend([flag, path, path].map(str::to_owned));
    }
    let cwd = run.cwd.as_str();
    args.extend(["--bind", cwd, cwd, "--chdir", cwd, "--clearenv"].map(str::to_owned));
    for var in &run.env {
        args.extend(["--setenv".to_owned(), var.name.clone(), var.value.clone()]);
    }
    if let AgentNet::Endpoint(bind) = &run.net {
        args.extend([
            "--ro-bind".to_owned(),
            bind.forwarder.as_str().to_owned(),
            INSIDE_FORWARDER.to_owned(),
            "--bind".to_owned(),
            bind.socket.as_str().to_owned(),
            INSIDE_SOCKET.to_owned(),
        ]);
    }
    args.push("--".to_owned());
    args.extend(inner(run));
    args
}

/// The words bubblewrap runs: the agent itself, or the forwarder that starts it.
fn inner(run: &AgentRun) -> Vec<String> {
    match &run.net {
        AgentNet::Endpoint(bind) => forwarder_words(bind, run.argv.words()),
        AgentNet::None | AgentNet::Host => run.argv.words().to_vec(),
    }
}

fn forwarder_words(bind: &EndpointBind, agent: &[String]) -> Vec<String> {
    let mut out = vec![
        INSIDE_FORWARDER.to_owned(),
        "run".to_owned(),
        "--listen".to_owned(),
        format!("127.0.0.1:{}", bind.port),
        "--socket".to_owned(),
        INSIDE_SOCKET.to_owned(),
        "--".to_owned(),
    ];
    out.extend(agent.iter().cloned());
    out
}

/// The directories to empty: those of `HIDDEN` that exist on this host.
pub fn present_hidden() -> Vec<&'static str> {
    HIDDEN
        .iter()
        .copied()
        .filter(|d| std::path::Path::new(d).is_dir())
        .collect()
}
