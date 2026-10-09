//! The registry reader and installer, over a fixture snapshot and archives built here. Nothing
//! reaches the network or the real home.

use docket_agents::fetch::{Fetch, FetchFault};
use docket_agents::hash::Sha256Hex;
use docket_agents::platform::Platform;
use docket_agents::record::Check;
use docket_agents::run::{Run, RunFault};
use docket_agents::slug::Slug;
use docket_agents::uv::PythonPackage;
use docket_agents::{AgentsDir, InstallFault, Snapshot, Standing, Want, install, standing};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

/// Serves archives by url.
struct Served(BTreeMap<String, Vec<u8>>);

impl Fetch for Served {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchFault> {
        self.0
            .get(url)
            .cloned()
            .ok_or_else(|| FetchFault(url.to_owned()))
    }
}

/// A package manager that "installs" a package with a program.
struct FakeNpm;

impl Run for FakeNpm {
    fn run(&self, _: &str, _: &[String], dir: &Path) -> Result<(), RunFault> {
        let pkg = dir.join("node_modules/@scope/tool");
        std::fs::create_dir_all(&pkg).map_err(|_| RunFault("npm".into()))?;
        std::fs::write(pkg.join("package.json"), r#"{"bin": {"tool-acp": "x.js"}}"#)
            .map_err(|_| RunFault("npm".into()))
    }
}

/// A `uv` that makes the environment and the program the package installs, and keeps each call.
#[derive(Default)]
struct FakeUv {
    calls: RefCell<Vec<Vec<String>>>,
    /// Whether the install leaves its program behind.
    leaves_program: bool,
}

impl Run for FakeUv {
    fn run(&self, program: &str, args: &[String], dir: &Path) -> Result<(), RunFault> {
        self.calls.borrow_mut().push(args.to_vec());
        let fail = || RunFault(program.to_owned());
        if program != "uv" {
            return Err(fail());
        }
        let bin = dir.join("venv/bin");
        if args.iter().any(|a| a == "venv") {
            std::fs::create_dir_all(&bin).map_err(|_| fail())?;
            return std::fs::write(bin.join("python"), b"").map_err(|_| fail());
        }
        if !self.leaves_program {
            return Ok(());
        }
        let requirement = args.last().ok_or_else(fail)?;
        let name = requirement.split("==").next().unwrap_or_default();
        std::fs::write(bin.join(name), b"#!/bin/sh\n").map_err(|_| fail())
    }
}

fn zip_of(name: &str, body: &[u8]) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    let options = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
    zip.start_file(name, options).expect("start");
    zip.write_all(body).expect("write");
    zip.finish().expect("finish");
    out.into_inner()
}

fn tar_gz_of(name: &str, body: &[u8]) -> Vec<u8> {
    let mut tarball = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(body.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    tarball
        .append_data(&mut header, name, body)
        .expect("append");
    let raw = tarball.into_inner().expect("tar");
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&raw).expect("gz");
    gz.finish().expect("gz end")
}

fn snapshot(sha: Option<&str>, version: &str) -> Snapshot {
    let key = Platform::current();
    let sha = sha.map_or(String::new(), |s| format!(r#", "sha256": "{s}""#));
    Snapshot::parse(&format!(
        r#"{{"version": "1", "extensions": [], "agents": [
        {{"id": "agy-acp", "name": "Agy", "version": "{version}", "extra": 1,
          "distribution": {{"binary": {{"{key}": {{"archive": "https://dl.test/agy-{version}.zip",
            "cmd": "./agy", "args": ["--uid="]{sha}}}}}}}}},
        {{"id": "claude-acp", "name": "Claude", "version": "0.1.0",
          "distribution": {{"npx": {{"package": "@scope/tool@0.1.0", "args": ["--x"]}}}}}},
        {{"id": "py", "name": "Py", "version": "1", "distribution": {{"uvx": {{"package": "py==1"}}}}}}
        ]}}"#,
        key = key.as_str()
    ))
    .expect("fixture")
}

fn served(version: &str, body: &[u8]) -> Served {
    let url = format!("https://dl.test/agy-{version}.zip");
    Served(BTreeMap::from([(url, zip_of("agy", body))]))
}

fn want<'a>(id: &'a str, version: &'a str) -> Want<'a> {
    Want { id, version }
}

#[test]
fn a_declared_digest_is_checked_and_the_program_unpacks_executable() {
    let net = served("1.0.0", b"#!/bin/sh\n");
    let digest = Sha256Hex::of(&net.0["https://dl.test/agy-1.0.0.zip"]);
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = AgentsDir::at(tmp.path());
    let snap = snapshot(Some(digest.as_str()), "1.0.0");
    let record = install(&dir, &snap, want("agy-acp", "1.0.0"), (&net, &FakeNpm)).expect("install");
    assert_eq!(record.check, Check::Declared);
    let launch = dir.launch("agy-acp", "1.0.0").expect("launch");
    assert_eq!(launch.args, ["--uid="]);
    assert!(launch.command.is_file());
    assert!(launch.home.is_dir());
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&launch.command)
        .expect("meta")
        .permissions()
        .mode();
    assert_eq!(mode & 0o111, 0o111);
}

#[test]
fn a_wrong_digest_installs_nothing() {
    let net = served("1.0.0", b"#!/bin/sh\n");
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = AgentsDir::at(tmp.path());
    let wrong = "0".repeat(64);
    let snap = snapshot(Some(&wrong), "1.0.0");
    let fault =
        install(&dir, &snap, want("agy-acp", "1.0.0"), (&net, &FakeNpm)).expect_err("refused");
    assert!(matches!(fault, InstallFault::Digest { .. }));
    assert!(dir.launch("agy-acp", "1.0.0").is_err());
}

#[test]
fn no_declared_digest_is_said_to_be_first_use() {
    let net = served("1.0.0", b"x");
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = AgentsDir::at(tmp.path());
    let record = install(
        &dir,
        &snapshot(None, "1.0.0"),
        want("agy-acp", "1.0.0"),
        (&net, &FakeNpm),
    )
    .expect("install");
    assert_eq!(record.check, Check::FirstUse);
    assert!(record.digest.is_some());
}

#[test]
fn only_the_listed_version_installs_and_a_newer_one_is_never_taken_alone() {
    let net = served("1.0.0", b"x");
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = AgentsDir::at(tmp.path());
    install(
        &dir,
        &snapshot(None, "1.0.0"),
        want("agy-acp", "1.0.0"),
        (&net, &FakeNpm),
    )
    .expect("install");
    // The registry moves on; nothing is fetched and the pin keeps working.
    let newer = snapshot(None, "1.1.0");
    assert_eq!(
        standing(&dir, &newer, "agy-acp"),
        Standing::Offered {
            offered: "1.1.0".into(),
            installed: vec!["1.0.0".into()]
        }
    );
    assert!(dir.launch("agy-acp", "1.0.0").is_ok());
    // Asking again for the installed version reaches nothing.
    let old = snapshot(None, "1.0.0");
    let none = Served(BTreeMap::new());
    assert!(install(&dir, &old, want("agy-acp", "1.0.0"), (&none, &FakeNpm)).is_ok());
    let stale =
        install(&dir, &newer, want("agy-acp", "0.9.0"), (&net, &FakeNpm)).expect_err("stale");
    assert!(matches!(stale, InstallFault::Version { .. }));
    assert_eq!(standing(&dir, &newer, "nope"), Standing::Unknown);
}

#[test]
fn a_node_package_goes_through_the_package_manager() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = AgentsDir::at(tmp.path());
    let snap = snapshot(None, "1.0.0");
    let net = Served(BTreeMap::new());
    let record =
        install(&dir, &snap, want("claude-acp", "0.1.0"), (&net, &FakeNpm)).expect("install");
    assert_eq!(record.check, Check::PackageManager);
    assert_eq!(record.command, "node_modules/.bin/tool-acp");
}

#[test]
fn a_python_package_goes_through_uv_into_its_own_directory() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = AgentsDir::at(tmp.path());
    let snap = snapshot(None, "1.0.0");
    let net = Served(BTreeMap::new());
    let uv = FakeUv {
        leaves_program: true,
        ..FakeUv::default()
    };
    let record = install(&dir, &snap, want("py", "1"), (&net, &uv)).expect("install");
    assert_eq!(record.check, Check::PackageManager);
    assert_eq!(record.command, "venv/bin/py");
    assert!(dir.launch("py", "1").expect("launch").command.is_file());
    let calls = uv.calls.borrow();
    assert_eq!(calls.len(), 2);
    assert!(calls[1].contains(&"py==1".to_owned()));
    assert!(calls.iter().all(|c| c.contains(&"--cache-dir".to_owned())));
    assert!(calls[0].contains(&"--no-python-downloads".to_owned()));
    assert!(calls[1].contains(&"pip".to_owned()));
}

#[test]
fn a_python_program_missing_after_uv_installs_nothing() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = AgentsDir::at(tmp.path());
    let snap = snapshot(None, "1.0.0");
    let net = Served(BTreeMap::new());
    let fault =
        install(&dir, &snap, want("py", "1"), (&net, &FakeUv::default())).expect_err("no program");
    assert_eq!(fault, InstallFault::NoProgram);
    assert!(dir.launch("py", "1").is_err());
    assert!(dir.versions(&Slug::parse("py").expect("slug")).is_empty());
}

#[test]
fn python_package_forms_are_read_and_direct_references_refused() {
    let pinned = PythonPackage::parse("minion-code@0.1.44").expect("at form");
    assert_eq!(pinned.requirement, "minion-code==0.1.44");
    assert_eq!(pinned.name, "minion-code");
    let extra = PythonPackage::parse("pkg[acp]==2").expect("extra");
    assert_eq!(extra.name, "pkg");
    assert!(PythonPackage::parse("git+https://example.test/x").is_err());
    assert!(PythonPackage::parse("pkg @ https://example.test/x.whl").is_err());
    assert!(PythonPackage::parse("../escape").is_err());
}

#[test]
fn a_tarball_unpacks_and_an_escaping_entry_is_refused() {
    let tmp = tempfile::tempdir().expect("tmp");
    let into = tmp.path().join("out");
    std::fs::create_dir_all(&into).expect("dir");
    docket_agents::unpack::unpack("a.tar.gz", &tar_gz_of("bin/agy", b"x"), &into).expect("unpack");
    assert!(into.join("bin/agy").is_file());
    let bad = zip_of("../escape", b"x");
    let fault = docket_agents::unpack::unpack("a.zip", &bad, &into).expect_err("escape");
    assert_eq!(fault, docket_agents::unpack::UnpackFault::Path);
    assert!(!tmp.path().join("escape").exists());
    let kind = docket_agents::unpack::unpack("a.tar.bz2", b"", &into).expect_err("kind");
    assert_eq!(kind, docket_agents::unpack::UnpackFault::Kind);
}

#[test]
fn names_that_could_climb_out_are_refused() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = AgentsDir::at(tmp.path());
    assert!(dir.launch("../x", "1").is_err());
    assert!(dir.launch("x", "..").is_err());
}

/// Every test file is one module of this binary: a `tests/*.rs` file would be a binary of its
/// own, and a `tests/it/` file or directory without a `mod` line would never run.
#[test]
fn every_test_file_is_reached_by_a_mod() {
    let tests = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let main = include_str!("main.rs");
    let mut stray = Vec::new();
    for entry in std::fs::read_dir(&tests).expect("tests dir").flatten() {
        let (name, path) = (entry.file_name(), entry.path());
        if path.extension().is_some_and(|e| e == "rs") {
            stray.push(format!("tests/{}: its own binary", name.to_string_lossy()));
        }
    }
    for entry in std::fs::read_dir(tests.join("it"))
        .expect("it dir")
        .flatten()
    {
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let is_module = path.is_dir() || path.extension().is_some_and(|e| e == "rs");
        let declared =
            main.contains(&format!("mod {stem};")) || main.contains(&format!("mod {stem} {{"));
        if is_module && stem != "main" && !declared {
            stray.push(format!("tests/it/{stem}: no `mod {stem};` in main.rs"));
        }
    }
    assert!(stray.is_empty(), "not reached: {stray:#?}");
}
