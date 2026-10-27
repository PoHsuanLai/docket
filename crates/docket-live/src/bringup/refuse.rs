//! What `up` refuses before it touches anything: a root that is a checkout or the home
//! directory, and a bus that is the real one. Pure: the facts about the current process arrive
//! as an [`Own`], so a table of cases tests it.

use std::path::{Component, Path, PathBuf};

/// The facts about the process the checks compare with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Own {
    /// `$HOME`.
    pub home: Option<PathBuf>,
    /// `$DBUS_SESSION_BUS_ADDRESS`: the real session bus, when set.
    pub session_bus: Option<String>,
    /// `$DBUS_SYSTEM_BUS_ADDRESS`, when set.
    pub system_bus: Option<String>,
}

impl Own {
    /// This process's own environment, as `main` hands it in.
    pub fn read(env: &impl Fn(&str) -> Option<String>) -> Self {
        let var = |name: &str| env(name).filter(|v| !v.is_empty());
        Self {
            home: var("HOME").map(PathBuf::from),
            session_bus: var("DBUS_SESSION_BUS_ADDRESS"),
            system_bus: var("DBUS_SYSTEM_BUS_ADDRESS"),
        }
    }
}

/// Why `up` will not start.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Refusal {
    /// The root is relative.
    #[error("--root {0} is not an absolute path")]
    RootNotAbsolute(PathBuf),
    /// The root has a `..` in it, so it cannot be compared with anything.
    #[error("--root {0} has a `..` in it")]
    RootDotDot(PathBuf),
    /// The root is `/` or the home directory.
    #[error(
        "--root {0} is too broad (the filesystem root or your home directory): name a scratch directory"
    )]
    RootTooBroad(PathBuf),
    /// The root is inside a git work tree.
    #[error(
        "--root {0} is inside the git checkout {1}: scratch state must live outside every repository"
    )]
    RootInGit(PathBuf, PathBuf),
    /// The bus is not a `unix:` address.
    #[error("--bus {0:?} is not a unix: bus address")]
    BusNotUnix(String),
    /// The bus is the one this process was started on.
    #[error(
        "--bus is this process's own bus ({0}): pass the demo's private bus, never the real one"
    )]
    BusIsOwn(String),
    /// The bus is where the real system or login-session bus lives.
    #[error("--bus {0} looks like the real system or session bus: pass the demo's private bus")]
    BusLooksReal(String),
}

/// Checks the scratch root.
pub fn check_root(root: &Path, own: &Own) -> Result<(), Refusal> {
    if !root.is_absolute() {
        return Err(Refusal::RootNotAbsolute(root.to_owned()));
    }
    if root.components().any(|c| c == Component::ParentDir) {
        return Err(Refusal::RootDotDot(root.to_owned()));
    }
    let broad = root.parent().is_none() || own.home.as_deref() == Some(root);
    if broad {
        return Err(Refusal::RootTooBroad(root.to_owned()));
    }
    match root.ancestors().find(|p| p.join(".git").exists()) {
        Some(checkout) => Err(Refusal::RootInGit(root.to_owned(), checkout.to_owned())),
        None => Ok(()),
    }
}

/// The socket paths an address names.
fn paths_of(address: &str) -> Vec<&str> {
    address
        .split(';')
        .filter_map(|one| one.strip_prefix("unix:"))
        .flat_map(|rest| rest.split(','))
        .filter_map(|pair| pair.strip_prefix("path="))
        .collect()
}

/// Whether a socket path is where the system bus or a login session's bus lives.
fn real_path(path: &str) -> bool {
    let session = path
        .strip_prefix("/run/user/")
        .and_then(|rest| rest.strip_suffix("/bus"))
        .is_some_and(|uid| !uid.is_empty() && uid.bytes().all(|b| b.is_ascii_digit()));
    session || path == "/var/run/dbus/system_bus_socket" || path == "/run/dbus/system_bus_socket"
}

/// Checks the bus address.
pub fn check_bus(bus: &str, own: &Own) -> Result<(), Refusal> {
    if !bus.starts_with("unix:") {
        return Err(Refusal::BusNotUnix(bus.to_owned()));
    }
    let mine = paths_of(bus);
    for real in [&own.session_bus, &own.system_bus].into_iter().flatten() {
        let shares = paths_of(real).iter().any(|p| mine.contains(p));
        if real == bus || shares {
            return Err(Refusal::BusIsOwn(real.clone()));
        }
    }
    match mine.iter().any(|p| real_path(p)) {
        true => Err(Refusal::BusLooksReal(bus.to_owned())),
        false => Ok(()),
    }
}
