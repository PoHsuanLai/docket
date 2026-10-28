//! The agent's environment, built from nothing. The launcher's own environment is never
//! inherited: the child gets a fixed base, the entry's plain `set` variables, and the one model
//! variable pair its route needs. The key is put in the child only, by the route:
//!
//! - endpoint (P4): the base URL and inferd's per-session token. Never the account's key.
//! - handoff (P2): the key under `key_env`.
//! - login: nothing. The agent has its own sign-in in its own state directory.

use crate::accounts::OpenedEndpoint;
use crate::config::Entry;
use crate::managed::claude_code_env;
use bulkhead::AbsPath;
use bulkhead::{EnvVar, SANDBOX_HOME, SANDBOX_PATH};
use porter_capability::EnvName;
use porter_core::SecretText;

/// What a route lent the child, kept alive until the process is built.
#[derive(Debug)]
pub enum Lent {
    /// Nothing (the login route).
    Nothing,
    /// An opened inferd endpoint (the endpoint route).
    Endpoint(OpenedEndpoint),
    /// A key (the handoff route).
    Key(SecretText),
}

/// The environment built for a child.
#[derive(Debug)]
pub struct Built {
    /// The whole environment.
    pub vars: Vec<EnvVar>,
}

fn var(name: &str, value: &str) -> EnvVar {
    EnvVar {
        name: name.to_owned(),
        value: value.to_owned(),
    }
}

fn put(vars: &mut Vec<EnvVar>, name: &EnvName, value: &str) {
    vars.retain(|v| v.name != name.as_str());
    vars.push(var(name.as_str(), value));
}

/// The environment for `entry` with what its route lent. The entry was checked against its
/// route when the file was read, so the variable names the lent value goes under are there.
pub fn child_env(entry: &Entry, lent: &Lent) -> Built {
    let home = entry.home.as_ref().map_or(SANDBOX_HOME, AbsPath::as_str);
    let mut vars = vec![
        var("PATH", SANDBOX_PATH),
        var("HOME", home),
        var("TMPDIR", "/tmp"),
        var("LANG", "C.UTF-8"),
        var("TERM", "dumb"),
    ];
    for (name, value) in &entry.set {
        put(&mut vars, name, value);
    }
    // The preset's variables come after the person's `set` and win over it: they are the point.
    if entry.claude_code {
        for (name, value) in claude_code_env() {
            vars.retain(|v| v.name != name);
            vars.push(var(name, value));
        }
    }
    match lent {
        Lent::Nothing => {}
        Lent::Endpoint(endpoint) => {
            if let Some(base) = &entry.base_url_env {
                put(&mut vars, base, &endpoint.base_url);
            }
            if let Some(key) = &entry.key_env {
                put(&mut vars, key, endpoint.token.expose());
            }
        }
        Lent::Key(secret) => {
            if let Some(key) = &entry.key_env {
                put(&mut vars, key, secret.expose());
            }
        }
    }
    Built { vars }
}
