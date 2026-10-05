//! Builds the two sibling daemons the acceptance runs as the packaged binaries: porter's `inferd`
//! and almanac's `memoryd` (with its `test-keys` feature, which the run needs because the private
//! bus has no Secret Service). Each is built from its own workspace and lock file, into a target
//! directory inside this build's (`<target>/accept-siblings/<repo>`), so the nextest archive's
//! target directory, which the jail binds, holds them and `cargo clean` removes them.
//!
//! The paths reach the tests as `ACCEPT_INFERD` and `ACCEPT_MEMORYD`. The nested cargo is the one
//! the outer cargo runs as (`$CARGO`, not the build-slot shim: the outer build already holds a
//! slot), with every cargo variable of this build scrubbed so the two do not share flags, a
//! jobserver or a target directory.

use std::path::{Path, PathBuf};
use std::process::Command;

struct Sibling {
    repo: &'static str,
    package: &'static str,
    features: &'static [&'static str],
}

const SIBLINGS: [Sibling; 2] = [
    Sibling {
        repo: "porter",
        package: "inferd",
        features: &[],
    },
    Sibling {
        repo: "almanac",
        package: "memoryd",
        features: &["test-keys"],
    },
];

fn var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set"))
}

fn build(sibling: &Sibling, root: &Path, target: &Path) -> PathBuf {
    let repo = root.join(sibling.repo);
    let mut command = Command::new(var("CARGO"));
    command
        .current_dir(&repo)
        .args(["build", "--locked", "--quiet", "-p", sibling.package])
        .args(["--bin", sibling.package]);
    if !sibling.features.is_empty() {
        command.args(["--features", &sibling.features.join(",")]);
    }
    std::env::vars()
        .map(|(name, _)| name)
        .filter(|name| name.starts_with("CARGO_") && name != "CARGO_HOME")
        .chain(["MAKEFLAGS", "MFLAGS", "RUSTFLAGS", "RUSTDOCFLAGS"].map(str::to_owned))
        .for_each(|name| {
            command.env_remove(name);
        });
    command.env("CARGO_TARGET_DIR", target);
    let status = command
        .status()
        .unwrap_or_else(|e| panic!("cargo for {}: {e}", sibling.repo));
    assert!(
        status.success(),
        "building {} in {} failed ({status})",
        sibling.package,
        repo.display()
    );
    println!("cargo:rerun-if-changed={}", repo.join("crates").display());
    println!("cargo:rerun-if-changed={}", repo.join("Cargo.lock").display());
    target.join("debug").join(sibling.package)
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let out = PathBuf::from(var("OUT_DIR"));
    // <target>/<profile>/build/<package>-<hash>/out
    let target = out
        .ancestors()
        .nth(4)
        .expect("OUT_DIR is inside a target directory")
        .join("accept-siblings");
    // The workspace's siblings sit beside the docket checkout: <root>/docket/crates/docket-accept.
    let root = PathBuf::from(var("CARGO_MANIFEST_DIR")).join("../../..");
    for sibling in &SIBLINGS {
        let binary = build(sibling, &root, &target.join(sibling.repo));
        let name = sibling.package.to_uppercase();
        println!("cargo:rustc-env=ACCEPT_{name}={}", binary.display());
    }
}
