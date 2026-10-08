//! The agent's environment, built from nothing. The launcher's own environment is never
//! inherited: the child gets a fixed base, the entry's plain `set` variables, and the one model
//! variable pair its route needs. The key is put in the child only, by the route:
//!
//! - endpoint (P4): the base URL and inferd's per-session token. Never the account's key.
//! - handoff (P2), `value`: the key under `key_env`. `file`: `<key_env>_FILE` names a tmpfs file
//!   and the key is in no environment.
//! - login: nothing. The agent has its own sign-in in its own state directory.

use crate::accounts::{KeyHandoff, OpenedEndpoint};
use crate::config::{Entry, Route};
use docket_core::AbsPath;
use docket_shell::{EnvVar, SANDBOX_HOME, SANDBOX_PATH};
use porter_core::capability::EnvName;

/// What a route gave the child.
#[derive(Debug)]
pub enum Lent<'a> {
    /// Nothing.
    Nothing,
    /// An inferd endpoint.
    Endpoint(&'a OpenedEndpoint),
    /// A key.
    Key(&'a KeyHandoff),
}

/// Why the environment could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EnvFault {
    /// What the route lent does not fit the entry (a key where none is read, a file variable
    /// whose name is too long).
    #[error("the entry cannot take what its route lent")]
    Mismatch,
}

/// The environment and the extra read-only path a file-delivered key needs.
#[derive(Debug)]
pub struct Built {
    /// The whole environment.
    pub vars: Vec<EnvVar>,
    /// A key file to bind read-only into the sandbox.
    pub key_file: Option<AbsPath>,
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

/// The environment for `entry` with what its route lent.
pub fn child_env(
    entry: &Entry,
    lent: &Lent<'_>,
    managed: Option<&AbsPath>,
) -> Result<Built, EnvFault> {
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
    if let (Some(profile), Some(file)) = (entry.profile, managed) {
        for (name, value) in profile.env(file) {
            vars.retain(|v| v.name != name);
            vars.push(var(name, &value));
        }
    }
    let mut key_file = None;
    match (entry.route, lent) {
        (Route::Login, Lent::Nothing) => {}
        (Route::Endpoint, Lent::Endpoint(endpoint)) => {
            let (Some(base), Some(key)) = (&entry.base_url_env, &entry.key_env) else {
                return Err(EnvFault::Mismatch);
            };
            put(&mut vars, base, &endpoint.base_url);
            put(&mut vars, key, endpoint.token.expose());
        }
        (Route::Handoff, Lent::Key(KeyHandoff::Value(secret))) => {
            let key = entry.key_env.as_ref().ok_or(EnvFault::Mismatch)?;
            put(&mut vars, key, secret.expose());
        }
        (Route::Handoff, Lent::Key(KeyHandoff::File(path))) => {
            let key = entry.key_env.as_ref().ok_or(EnvFault::Mismatch)?;
            let name = EnvName::parse(&format!("{}_FILE", key.as_str()))
                .map_err(|_| EnvFault::Mismatch)?;
            put(&mut vars, &name, path.as_str());
            key_file = Some(path.clone());
        }
        _ => return Err(EnvFault::Mismatch),
    }
    Ok(Built { vars, key_file })
}
