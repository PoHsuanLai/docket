//! `voiced-callers.toml`: the one process that begins utterances is the shell's unit. The shipped
//! file is `dist/voiced-callers.toml`; the system's copy under `/etc/quire` and the person's
//! under their configuration directory are laid over each other
//! (`docket_dbus::callers::read_table`), and only the system's can give a fixed role.

use docket_dbus::callers::{CallerFileError, CallerTable, read_table, table_from_toml};

/// The file name in both directories.
pub const FILE: &str = "voiced-callers.toml";

const SHIPPED: &str = include_str!("../../../dist/voiced-callers.toml");

/// The table the package ships. A shipped file that does not read is an empty table: nobody is
/// identified, which is the strict way to fail.
pub fn shipped() -> CallerTable {
    table_from_toml(SHIPPED).unwrap_or_default()
}

/// The table of this machine: `/etc/quire/voiced-callers.toml` with the person's file over it.
pub fn read(env: &impl Fn(&str) -> Option<String>) -> Result<CallerTable, CallerFileError> {
    read_table(FILE, env)
}
