//! Shared by the live-harness tests: where the daemon binaries are. The daemons are
//! `docket-accept`'s (`accept-intentd` and the rest), built into the same target directory as this
//! test: `cargo build -p docket-accept --bins` builds them (a workspace test run does).
#![allow(dead_code)]

use docket_accept::world::{Binaries, Cassette};
use std::path::PathBuf;

/// The directory the workspace's programs are built into: two levels above this test binary
/// (`<target>/<profile>/deps/it-<hash>`).
fn built_in() -> PathBuf {
    let exe = std::env::current_exe().expect("this test's path");
    exe.parent()
        .and_then(std::path::Path::parent)
        .expect("the target directory")
        .to_owned()
}

/// A program of `docket-accept` built beside this test.
pub fn accept_bin(name: &str) -> PathBuf {
    built_in().join(name)
}

pub fn binaries() -> Binaries {
    // Every test starts here: a test that waits for ever ends the process after 20 minutes.
    docket_testbus::hang_guard::arm();
    Binaries::beside(&accept_bin("docket-live"), &|_: &str| None)
}

/// Flow (a), the person allows: search, contact search, forward, the closing words.
pub const FLOW_A: Cassette = Cassette(include_str!(
    "../../../../dev/accept/cassettes/flow-a.jsonl"
));

/// A command line as the words the parser takes.
pub fn words(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_owned).collect()
}
