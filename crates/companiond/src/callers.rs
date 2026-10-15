//! `companiond-callers.toml`: the one process that speaks for the person with the whole voice is
//! the shell's unit. The shipped file is `dist/companiond-callers.toml`; the system's copy under
//! `/etc/quire` and the person's under their configuration directory are laid over each other
//! (`docket_dbus::callers::read_table`), and only the system's can give a fixed role.

use docket_dbus::callers::{
    CallerFileError, CallerTable, read_table, read_table_at, table_from_toml,
};
use std::path::Path;

/// The file name in both directories.
pub const FILE: &str = "companiond-callers.toml";

const SHIPPED: &str = include_str!("../../../dist/companiond-callers.toml");

/// The table the package ships. A shipped file that does not read is an empty table: nobody is
/// identified, which is the strict way to fail.
pub fn shipped() -> CallerTable {
    table_from_toml(SHIPPED).unwrap_or_default()
}

/// The table of this machine: `/etc/quire/companiond-callers.toml` with the person's file over it.
pub fn read(env: &impl Fn(&str) -> Option<String>) -> Result<CallerTable, CallerFileError> {
    read_table(FILE, env)
}

/// As [`read`], with the system's file at `system` (the acceptance harness's own system).
pub fn read_at(
    system: &Path,
    env: &impl Fn(&str) -> Option<String>,
) -> Result<CallerTable, CallerFileError> {
    read_table_at(system, FILE, env)
}
