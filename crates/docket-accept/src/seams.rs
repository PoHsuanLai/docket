//! The test-only seams of the daemons, read from the environment by the harness's mains alone.
//! The shipped daemons carry none of this: their `main` passes the defaults (the system's `/proc`,
//! no model tap) and no environment variable can change either. The world sets these variables
//! on the processes it spawns (`world::spawn`).

use docket_dbus::tap::Tap;
use std::ffi::OsString;
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

/// A non-empty value of `name`.
fn set(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|value| !value.is_empty())
}

/// The model tap the world asked for: a file when [`TRACE_VAR`] names one, else off.
pub fn trace() -> Tap {
    set(TRACE_VAR).map_or_else(Tap::off, |path| Tap::trace_file(PathBuf::from(path)))
}

/// The fixture `/proc` named by the variable `name`, if the world set one.
pub fn proc_root(name: &str) -> Option<PathBuf> {
    set(name).map(PathBuf::from)
}

/// The file named by the variable `name`, if the world set one: a system callers file.
pub fn system_callers(name: &str) -> Option<PathBuf> {
    set(name).map(PathBuf::from)
}
