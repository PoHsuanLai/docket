//! `intentd-callers.toml`: which unit is which app, and with what role. The shipped file is
//! `dist/intentd-callers.toml`; the system's copy under `/etc/quire` and the person's under their
//! configuration directory are laid over each other (`docket_dbus::callers::read_table`), and only
//! the system's can give a fixed role.

use docket_dbus::callers::{
    CallerFileError, CallerTable, read_table, read_table_at, table_from_toml,
};
use std::path::Path;

/// The file name in both directories.
pub const FILE: &str = "intentd-callers.toml";

const SHIPPED: &str = include_str!("../../../dist/intentd-callers.toml");

/// The table the package ships. A shipped file that does not read is an empty table: nobody is
/// identified, which is the strict way to fail.
pub fn shipped() -> CallerTable {
    table_from_toml(SHIPPED).unwrap_or_default()
}

/// The table of this machine: `/etc/quire/intentd-callers.toml` with the person's file over it.
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

#[cfg(test)]
mod tests {
    use super::*;
    use docket_dbus::callers::Role;

    #[test]
    fn the_shipped_file_reads_and_gives_the_shell_the_sheet_host_role_by_its_unit() {
        let table = shipped();
        let shell = table
            .resolve_unit("sill.service")
            .expect("the shell's unit");
        assert_eq!(shell.app.name.as_str(), "org.quire.Shell");
        assert_eq!(shell.role, Role::SheetHost);
        assert!(table.resolve_unit("sill-shell.scope").is_none());
    }
}
