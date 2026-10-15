//! What `up` puts in the root before any daemon starts: the directory layout, the configuration
//! files, and the fake proc root that names each caller.

use crate::world::{Cgroup, MEMORY_CALLERS, SPACES};
use std::path::Path;

/// The inferd rows `up` always writes, ahead of anything the person adds: no engine program is
/// named (so none can be started or attached) and no local runtime port is probed (Ollama,
/// llama.cpp, LM Studio). A local model on a small GPU once broke the owner's display; this
/// stack never configures one.
pub const NO_LOCAL_ENGINE: &str =
    "[engines]\n\n[probe]\nollama = []\nllama_cpp = []\nlm_studio = []\n";

/// Tables the person's extra rows may not hold: `up` owns them.
const OWNED_TABLES: [&str; 3] = ["engines", "probe", "callers"];

/// Why the person's extra `inferd.toml` rows are refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ExtraFault {
    /// The text is not TOML.
    #[error("not valid TOML: {0}")]
    Toml(String),
    /// It holds a table that `up` owns.
    #[error(
        "it holds a [{0}] table; docket-bringup writes that one (no local engine, fixed callers)"
    )]
    Owned(&'static str),
}

/// The `inferd.toml` of a root: the fixed rows, then the person's `extra` rows, then the callers.
pub fn inferd_toml(root: &Path, extra: &str) -> Result<String, ExtraFault> {
    let table: toml::Table = extra
        .parse()
        .map_err(|e: toml::de::Error| ExtraFault::Toml(e.to_string()))?;
    if let Some(owned) = OWNED_TABLES.iter().find(|name| table.contains_key(**name)) {
        return Err(ExtraFault::Owned(owned));
    }
    let body = format!("{NO_LOCAL_ENGINE}\n{extra}");
    let model = crate::world::ModelSource::Live(body);
    let toml = crate::world::inferd_toml(root, &model, None);
    Ok(toml.replacen("[callers.apps]", SHELL_CALLERS, 1))
}

/// The rows an install gives inferd beside the apps: the shell's unit (here `sill.service`, the
/// unit the fake proc root puts the shell in) and the daemons that may choose where the
/// assistant runs.
const SHELL_CALLERS: &str = "[callers]\nshell = [\"sill.service\"]\nplaces = [\"companiond.service\", \"readerd.service\", \"intentd.service\"]\n\n[callers.apps]";

/// The directories of a root, as a World makes them.
pub fn layout(root: &Path) -> std::io::Result<()> {
    for sub in ["logs", "data", "config", "cache", "run", "keys", "proc"] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    std::fs::set_permissions(
        root.join("run"),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )?;
    std::fs::set_permissions(
        root.join("keys"),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
}

fn put(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)
}

/// Puts process `pid` in the fake proc root `<root>/proc`, in `cgroup`; in a service unit it is
/// that unit's main process (`proc/units/<name>.service`). The same rows `world::place` writes,
/// with the errors returned.
pub fn place(root: &Path, pid: u32, cgroup: Cgroup<'_>) -> std::io::Result<()> {
    put(&root.join(format!("proc/{pid}/cgroup")), &cgroup.line())?;
    match cgroup {
        Cgroup::Unit(name) => put(
            &root.join(format!("proc/units/{name}.service")),
            &pid.to_string(),
        ),
        _ => Ok(()),
    }
}

/// The unit the shell runs in.
pub const SHELL_UNIT: &str = "sill";

/// Writes the configuration files a World writes (memoryd's callers and Spaces, inferd's
/// configuration), and puts the shell in the fake proc root. intentd and companiond read no
/// file: without one they run on the configuration they ship.
pub fn files(root: &Path, shell_pid: u32, inferd_toml: &str) -> std::io::Result<()> {
    place(root, shell_pid, Cgroup::Unit(SHELL_UNIT))?;
    put(
        &root.join("config/quire/memory-callers.toml"),
        MEMORY_CALLERS,
    )?;
    put(&root.join("data/quire/memory/spaces.toml"), SPACES)?;
    crate::world::write_system_callers(root)?;
    put(&root.join("config/quire/inferd.toml"), inferd_toml)
}
