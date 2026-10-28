//! The test-only seams of the daemons, read from the environment by the harness's mains alone: each
//! takes the environment `main` hands in.
//! The shipped daemons carry none of this: their `main` passes the defaults (the system's `/proc`,
//! no model tap) and no environment variable can change either. The world sets these variables
//! on the processes it spawns (`world::spawn`).

use docket_dbus::InferWatch;
use docket_trace::Tap;
use porter_callers::ProcRoot;
use std::path::PathBuf;

/// The variable that names the file the model tap appends to (prompts go to disk).
pub const TRACE_VAR: &str = "DOCKET_MODEL_TRACE";

/// The variable that moves the `/proc` intentd reads its callers from.
pub const INTENTD_PROC_ROOT_VAR: &str = "INTENTD_PROC_ROOT";

/// The variable that moves the `/proc` companiond reads its callers from.
pub const COMPANIOND_PROC_ROOT_VAR: &str = "COMPANIOND_PROC_ROOT";

/// The variable that names the system's callers file intentd reads (`intentd-callers.toml`): the
/// harness is its own system, and a fixed role (the shell's) comes from no file of the person's.
pub const INTENTD_SYSTEM_CALLERS_VAR: &str = "INTENTD_SYSTEM_CALLERS";

/// The variable that names the system's callers file companiond reads.
pub const COMPANIOND_SYSTEM_CALLERS_VAR: &str = "COMPANIOND_SYSTEM_CALLERS";

/// The variable that moves the `/proc` readerd reads its callers from.
pub const READERD_PROC_ROOT_VAR: &str = "READERD_PROC_ROOT";

/// The variable that names the system's callers file readerd reads.
pub const READERD_SYSTEM_CALLERS_VAR: &str = "READERD_SYSTEM_CALLERS";

/// The variable that moves the `/proc` docket-acp reads its callers from.
pub const ACP_PROC_ROOT_VAR: &str = "DOCKET_ACP_PROC_ROOT";

/// The variable that names the system's callers file docket-acp reads.
pub const ACP_SYSTEM_CALLERS_VAR: &str = "DOCKET_ACP_SYSTEM_CALLERS";

/// The variable that names the agent host's fixture `/proc`.
pub const AGENT_PROC_ROOT_VAR: &str = "DOCKET_AGENT_PROC_ROOT";

/// The variable that names the agent host's system callers file.
pub const AGENT_SYSTEM_CALLERS_VAR: &str = "DOCKET_AGENT_SYSTEM_CALLERS";

/// The variable that, set, bounds the agents' sandboxes per process: a world with no systemd user
/// manager (the jail) has no scope to put them in.
pub const AGENT_PER_PROCESS_LIMITS_VAR: &str = "DOCKET_AGENT_RLIMIT";

/// A non-empty value of `name`.
fn set(env: &impl Fn(&str) -> Option<String>, name: &str) -> Option<String> {
    env(name).filter(|value| !value.is_empty())
}

/// The model tap the world asked for: a file when [`TRACE_VAR`] names one, else off.
pub fn trace(env: &impl Fn(&str) -> Option<String>) -> InferWatch {
    set(env, TRACE_VAR).map_or_else(InferWatch::off, |path| {
        Tap::trace_file(PathBuf::from(path)).watch()
    })
}

/// The `/proc` a daemon reads: the fixture tree named by the variable `name` if the world set
/// one, else the system's.
pub fn proc_root(env: &impl Fn(&str) -> Option<String>, name: &str) -> ProcRoot {
    set(env, name).map_or(ProcRoot::Unset, |dir| {
        ProcRoot::Honoured(PathBuf::from(dir))
    })
}

/// The file named by the variable `name`, if the world set one: a system callers file.
pub fn system_callers(env: &impl Fn(&str) -> Option<String>, name: &str) -> Option<PathBuf> {
    set(env, name).map(PathBuf::from)
}

/// Whether the variable `name` is set (and not empty).
pub fn switched_on(env: &impl Fn(&str) -> Option<String>, name: &str) -> bool {
    set(env, name).is_some()
}
