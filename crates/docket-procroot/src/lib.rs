//! What the daemons read from a `/proc` tree. Which tree is the daemon's choice (porter's
//! `ProcRoot`: the system's, or a fixture in a test build); this crate reads under the directory
//! it is handed, so a test builds a scratch one.

use docket_wire::TerminalScope;
use porter_core::CgroupPath;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// The terminal scope that process `pid`'s cgroup (`<root>/<pid>/cgroup`) ends in, if it is a
/// terminal's child. A process whose cgroup cannot be read is in none.
pub fn terminal_scope(root: &Path, pid: u32) -> Option<TerminalScope> {
    let text =
        std::fs::read_to_string(root.join(pid.to_string()).join("cgroup")).unwrap_or_default();
    let cgroup = CgroupPath::from_proc(&text).ok()?;
    TerminalScope::from_cgroup(cgroup.as_str()).ok()
}

/// The user this process runs as, read from the system's `/proc/self` (never from a fixture
/// tree: it is the daemon itself that is asked about).
pub fn own_uid() -> Option<u32> {
    std::fs::metadata("/proc/self").ok().map(|m| m.uid())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(root: &Path, pid: u32, leaf: &str) {
        let at = root.join(pid.to_string());
        std::fs::create_dir_all(&at).expect("dir");
        std::fs::write(
            at.join("cgroup"),
            format!("0::/user.slice/user-1000.slice/user@1000.service/app.slice/{leaf}\n"),
        )
        .expect("cgroup");
    }

    #[test]
    fn a_cgroup_makes_a_terminal_only_for_a_terminal_scope() {
        let rows: [(&str, bool); 9] = [
            ("vte-spawn-1b2c.scope", true),
            ("tmux-spawn-9.scope", true),
            ("session-3.scope", true),
            ("app-org.kde.konsole-123.scope", false),
            ("intentd.service", false),
            ("vte-spawn-1b2c.service", false),
            ("session-3.slice", false),
            ("run-r4242.scope", false),
            ("dbus-broker.service", false),
        ];
        for (leaf, expected) in rows {
            let root = tempfile::tempdir().expect("scratch");
            put(root.path(), 41, leaf);
            let scope = terminal_scope(root.path(), 41);
            assert_eq!(scope.is_some(), expected, "{leaf}");
            if let Some(scope) = scope {
                assert_eq!(scope.as_str(), leaf);
            }
        }
    }

    #[test]
    fn a_process_with_no_readable_cgroup_is_in_no_terminal() {
        let root = tempfile::tempdir().expect("scratch");
        assert_eq!(terminal_scope(root.path(), 7), None, "no file");
        let at = root.path().join("8");
        std::fs::create_dir_all(&at).expect("dir");
        std::fs::write(at.join("cgroup"), "not a cgroup line").expect("file");
        assert_eq!(terminal_scope(root.path(), 8), None, "not a cgroup line");
    }
}
