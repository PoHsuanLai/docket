//! The run of an agent's own sign-in in a terminal, built the way the agent itself would be run:
//! the entry's command and arguments with the login added, the entry's variables built from
//! nothing, the entry's binds (its program, its own state), and the sandbox's own network for a
//! sign-in, which has to reach the site that signs the person in. Nothing is lent: whatever the
//! person signs in with is the agent's, kept in the agent's own state, never read here.

use crate::config::Entry;
use crate::env::{Lent, child_env};
use crate::spawn::binds;
use bulkhead::AbsPath;
use bulkhead::{AgentNet, AgentRun, Argv, EnvVar, NetworkMode};
use std::collections::BTreeMap;

/// The variable a terminal's program reads to know what it is talking to.
const TERM: &str = "TERM";
/// What a real terminal says it is.
const TERMINAL_KIND: &str = "xterm-256color";

fn set(vars: &mut Vec<EnvVar>, name: &str, value: &str) {
    vars.retain(|v| v.name != name);
    vars.push(EnvVar {
        name: name.to_owned(),
        value: value.to_owned(),
    });
}

/// The confined run of `entry`'s command with `args` added and `env` set over its variables, in
/// `folder`. None for an entry whose model comes from a key or an endpoint (it has no sign-in of
/// its own to run), or one whose paths do not fit the sandbox.
pub fn login_run(
    entry: &Entry,
    args: &[String],
    env: &BTreeMap<String, String>,
    folder: &AbsPath,
) -> Option<AgentRun> {
    let mut vars = child_env(entry, &Lent::Nothing).ok()?.vars;
    set(&mut vars, TERM, TERMINAL_KIND);
    for (name, value) in env {
        set(&mut vars, name, value);
    }
    let all: Vec<String> = entry.args.iter().chain(args).cloned().collect();
    let argv = Argv::new(entry.command.as_str(), &all)?;
    let net = AgentNet::of(NetworkMode::Host, None).ok()?;
    let run = AgentRun::new(argv, folder.clone(), vars, net);
    let binds = binds(entry, None);
    Some(binds.into_iter().fold(run, |run, bind| run.with_bind(bind)))
}
