//! Where the daemon reads `/proc/<pid>/cgroup`: the system's `/proc`, unless a test build is
//! told otherwise.
//!
//! `INTENTD_PROC_ROOT=<dir>` makes the daemon read `<dir>/<pid>/cgroup` for its callers, only in
//! a build with the `test-proc-root` feature (off by default, never in a release or dist build).
//! Without the feature the variable is ignored and the daemon says so, so an environment cannot
//! make a production daemon believe a caller is somebody else. It exists so an acceptance run
//! can present its test processes as a terminal or a unit.

use std::path::PathBuf;

/// The environment variable the switch reads.
pub const PROC_ROOT_VAR: &str = "INTENTD_PROC_ROOT";

/// Whether this build has the fixture `/proc` switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestProcRoot {
    /// Built with the `test-proc-root` feature.
    Built,
    /// The normal build.
    NotBuilt,
}

impl TestProcRoot {
    /// What this build is.
    pub const THIS_BUILD: TestProcRoot = if cfg!(feature = "test-proc-root") {
        TestProcRoot::Built
    } else {
        TestProcRoot::NotBuilt
    };
}

/// The `/proc` the daemon reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcRoot {
    /// The system's `/proc`.
    System,
    /// The system's `/proc`, though the variable was set: this build ignores it.
    SystemIgnoring(String),
    /// A fixture tree (test builds only).
    Fixture(PathBuf),
}

/// Chooses the root, pure over the variable's value and the build.
pub fn proc_root_choice(var: Option<&str>, build: TestProcRoot) -> ProcRoot {
    match (var, build) {
        (None | Some(""), _) => ProcRoot::System,
        (Some(value), TestProcRoot::NotBuilt) => ProcRoot::SystemIgnoring(value.to_owned()),
        (Some(value), TestProcRoot::Built) => ProcRoot::Fixture(PathBuf::from(value)),
    }
}

impl ProcRoot {
    /// The directory to read.
    pub fn path(&self) -> PathBuf {
        match self {
            ProcRoot::System | ProcRoot::SystemIgnoring(_) => PathBuf::from("/proc"),
            ProcRoot::Fixture(dir) => dir.clone(),
        }
    }

    /// What the daemon says on standard error about the choice, if anything.
    pub fn said(&self) -> Option<String> {
        match self {
            ProcRoot::System => None,
            ProcRoot::SystemIgnoring(_) => Some(format!(
                "{PROC_ROOT_VAR} is set but this build has no test-proc-root feature; ignoring it and reading /proc"
            )),
            ProcRoot::Fixture(dir) => Some(format!(
                "TEST BUILD: reading callers from the proc root {}, not /proc",
                dir.display()
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_choice_is_pure_over_the_variable_and_the_build() {
        let fixture = |p: &str| ProcRoot::Fixture(PathBuf::from(p));
        let rows = [
            (None, TestProcRoot::Built, ProcRoot::System),
            (None, TestProcRoot::NotBuilt, ProcRoot::System),
            (Some(""), TestProcRoot::Built, ProcRoot::System),
            (
                Some("/x"),
                TestProcRoot::NotBuilt,
                ProcRoot::SystemIgnoring("/x".into()),
            ),
            (Some("/x"), TestProcRoot::Built, fixture("/x")),
        ];
        for (var, build, expected) in rows {
            assert_eq!(proc_root_choice(var, build), expected, "{var:?} {build:?}");
        }
    }

    #[test]
    fn a_choice_that_is_not_the_system_says_so_and_only_a_test_build_reads_the_fixture() {
        assert_eq!(ProcRoot::System.said(), None);
        let ignored = ProcRoot::SystemIgnoring("/x".into());
        assert_eq!(ignored.path(), Path::new("/proc"));
        assert!(ignored.said().is_some_and(|l| l.contains("ignoring it")));
        let fixture = ProcRoot::Fixture(PathBuf::from("/x"));
        assert_eq!(fixture.path(), Path::new("/x"));
        assert!(
            fixture
                .said()
                .is_some_and(|l| l.contains("TEST BUILD") && l.contains("/x"))
        );
    }

    #[test]
    fn this_build_of_the_tests_is_not_a_test_proc_root_build_unless_the_feature_says_so() {
        assert_eq!(
            TestProcRoot::THIS_BUILD == TestProcRoot::Built,
            cfg!(feature = "test-proc-root")
        );
    }
}
