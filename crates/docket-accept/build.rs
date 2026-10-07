//! Builds the two daemons the acceptance runs as the packaged binaries: porter's `inferd`
//! and almanac's `memoryd` (with `test-keys`, because the private bus has no Secret Service, and `test-proc-root`
//! on both, so callers are named from a fake proc root). Each is built from its own workspace and lock file, into a target
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
        features: &["test-proc-root"],
    },
    Sibling {
        repo: "almanac",
        package: "memoryd",
        features: &["test-keys", "test-proc-root"],
    },
];

fn var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set"))
}

/// The `(git url, rev)` the workspace manifest pins for `repo`, read from its `<repo>-core`-style
/// dependency lines: the one place the pin lives.
fn pin(manifest: &str, repo: &str) -> (String, String) {
    let needle = format!("git = \"https://github.com/PoHsuanLai/{repo}\"");
    let line = manifest
        .lines()
        .find(|l| l.contains(&needle))
        .unwrap_or_else(|| panic!("the workspace manifest pins no git rev of {repo}"));
    let rev = line
        .split("rev = \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap_or_else(|| panic!("no rev on the {repo} line"));
    (
        format!("https://github.com/PoHsuanLai/{repo}"),
        rev.to_owned(),
    )
}

fn build(sibling: &Sibling, manifest: &str, target: &Path) -> PathBuf {
    let local = std::env::var(format!("ACCEPT_{}_DIR", sibling.repo.to_uppercase())).ok();
    println!(
        "cargo:rerun-if-env-changed=ACCEPT_{}_DIR",
        sibling.repo.to_uppercase()
    );
    let mut command = Command::new(var("CARGO"));
    command.args(["install", "--locked", "--quiet", "--force", "--debug"]);
    command.args(["--root"]).arg(target);
    command.args(["--target-dir"]).arg(target.join("build"));
    match &local {
        Some(dir) => command
            .args(["--path"])
            .arg(Path::new(dir).join("crates").join(sibling.package)),
        None => {
            let (url, rev) = pin(manifest, sibling.repo);
            command
                .args(["--git", &url, "--rev", &rev])
                .arg(sibling.package)
        }
    };
    command.args(["--bin", sibling.package]);
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
        "building {} of {} failed ({status})",
        sibling.package,
        sibling.repo
    );
    if let Some(dir) = &local {
        println!(
            "cargo:rerun-if-changed={}",
            Path::new(dir).join("crates").display()
        );
    }
    target.join("bin").join(sibling.package)
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
    let workspace_manifest = PathBuf::from(var("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    println!("cargo:rerun-if-changed={}", workspace_manifest.display());
    let manifest = std::fs::read_to_string(&workspace_manifest)
        .unwrap_or_else(|e| panic!("reading {}: {e}", workspace_manifest.display()));
    for sibling in &SIBLINGS {
        let binary = build(sibling, &manifest, &target.join(sibling.repo));
        let name = sibling.package.to_uppercase();
        println!("cargo:rustc-env=ACCEPT_{name}={}", binary.display());
    }
}
