//! `dist/install-dev.sh` in a jail: HOME, every XDG directory and the prefix are directories of a
//! scratch tempdir, the binaries are fake files named by `INSTALL_DEV_BIN_DIR` (nothing builds,
//! nothing is compiled), and the environment is cleared, so the person's real `~/.config`,
//! `~/.local` and `/etc` are never named. The script reads the sibling checkouts' `dist` files, so the tests that
//! install skip (and say so) when porter, almanac and stoker are not checked out beside docket: a plain
//! clone builds with no sibling, and this is a developer tool.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BINARIES: [&str; 9] = [
    "intentd",
    "companiond",
    "readerd",
    "actions-mcp",
    "quire-do",
    "docket-eval",
    "memoryd",
    "inferd",
    "accountd",
];

/// The script's sources are the sibling checkouts; without them there is nothing to install from.
fn siblings_present() -> bool {
    let beside = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let here = ["porter/dist", "almanac/dbus", "stoker/catalog"]
        .iter()
        .all(|dir| beside.join(dir).is_dir());
    if !here {
        eprintln!("skipped: porter, almanac and stoker are not checked out beside docket");
    }
    here
}

struct Jail {
    dir: tempfile::TempDir,
}

impl Jail {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("scratch");
        let fakes = dir.path().join("fakes");
        std::fs::create_dir_all(&fakes).expect("fakes");
        for name in BINARIES {
            std::fs::write(fakes.join(name), format!("#!/bin/sh\necho fake {name}\n"))
                .expect("fake");
        }
        std::fs::create_dir_all(dir.path().join("home")).expect("home");
        Self { dir }
    }

    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    fn config(&self) -> PathBuf {
        self.home().join(".config")
    }

    fn prefix(&self) -> PathBuf {
        self.home().join(".local")
    }

    fn script(&self) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/install-dev.sh")
    }

    fn run(&self, args: &[&str]) -> Output {
        let home = self.home();
        Command::new("bash")
            .arg(self.script())
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("PREFIX", home.join(".local"))
            .env("INSTALL_DEV_BIN_DIR", self.dir.path().join("fakes"))
            .output()
            .expect("bash runs")
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(
            out.status.success(),
            "{args:?}: {:?}\nstdout: {text}\nstderr: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
        text
    }

    /// Every file and directory under HOME, relative to it.
    fn tree(&self) -> BTreeSet<String> {
        fn walk(root: &Path, dir: &Path, into: &mut BTreeSet<String>) {
            for entry in std::fs::read_dir(dir).expect("dir") {
                let path = entry.expect("entry").path();
                let rel = path
                    .strip_prefix(root)
                    .expect("under")
                    .display()
                    .to_string();
                if path.is_dir() {
                    into.insert(format!("{rel}/"));
                    walk(root, &path, into);
                } else {
                    into.insert(rel);
                }
            }
        }
        let mut found = BTreeSet::new();
        walk(&self.home(), &self.home(), &mut found);
        found
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.home().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
    }
}

#[test]
fn an_install_puts_every_piece_where_the_session_reads_it() {
    if !siblings_present() {
        return;
    }
    let jail = Jail::new();
    let out = jail.ok(&[]);
    let tree = jail.tree();
    let has = |rel: &str| assert!(tree.contains(rel), "missing {rel}\n{tree:#?}");
    for name in BINARIES {
        has(&format!(".local/bin/{name}"));
    }
    for unit in [
        "intentd",
        "companiond",
        "readerd",
        "actions-mcp",
        "inferd",
        "accountd",
        "memoryd",
    ] {
        has(&format!(".config/systemd/user/{unit}.service"));
    }
    for name in [
        "Companion1",
        "Intents1",
        "Reader1",
        "Accounts1",
        "Inference1",
        "Memory1",
    ] {
        has(&format!(
            ".local/share/dbus-1/services/org.quire.{name}.service"
        ));
    }
    has(".local/share/quire/skills/desktop-basics/SKILL.md");
    has(".local/share/quire/settings/docket.settings.toml");
    has(".local/share/quire/settings/almanac.settings.toml");
    has(".local/share/quire/settings/inferd.settings.toml");
    has(".local/share/porter/providers/openrouter.toml");
    has(".local/share/stoker/catalog/claude-haiku-4.5.toml");
    for config in [
        "intentd",
        "companiond",
        "inferd",
        "memory-callers",
        "actions-mcp",
    ] {
        has(&format!(".config/quire/{config}.toml"));
    }
    // Offline unless asked: no drop-in for inferd.
    assert!(
        !tree.iter().any(|p| p.contains("inferd.service.d")),
        "{tree:#?}"
    );
    has(".local/state/quire-dev/install-dev.manifest");
    // Nothing for voiced (a skeleton), syncd or cuad, and nothing outside HOME.
    assert!(
        !tree
            .iter()
            .any(|p| p.contains("voiced") || p.contains("syncd")),
        "{tree:#?}"
    );
    assert!(
        out.contains("sudo install -D -m644 ")
            && out.contains("/dist/callers.toml /etc/porter/callers.toml"),
        "{out}"
    );
}

#[test]
fn the_exec_lines_name_the_prefix_and_a_home_prefix_keeps_the_binary_reachable() {
    if !siblings_present() {
        return;
    }
    let jail = Jail::new();
    jail.ok(&[]);
    let bin = jail.prefix().join("bin");
    let bin = bin.display();
    for (unit, name) in [
        ("companiond", "companiond"),
        ("intentd", "intentd"),
        ("inferd", "inferd"),
        ("memoryd", "memoryd"),
    ] {
        let text = jail.read(&format!(".config/systemd/user/{unit}.service"));
        assert!(
            text.contains(&format!("\nExecStart={bin}/{name}")),
            "{unit}\n{text}"
        );
        let code: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
        assert!(
            !code.iter().any(|l| l.contains("/usr/libexec")),
            "{unit}\n{text}"
        );
    }
    let dbus = jail.read(".local/share/dbus-1/services/org.quire.Companion1.service");
    assert!(dbus.contains(&format!("Exec={bin}/companiond\n")), "{dbus}");
    // With the binary under HOME, `ProtectHome=yes` would hide it from its own unit.
    for unit in ["companiond", "readerd"] {
        let text = jail.read(&format!(".config/systemd/user/{unit}.service"));
        assert!(!text.contains("\nProtectHome=yes"), "{unit}");
        assert!(text.contains("\nProtectHome=read-only"), "{unit}");
    }
    let mode = std::os::unix::fs::PermissionsExt::mode(
        &std::fs::metadata(jail.prefix().join("bin/inferd"))
            .expect("meta")
            .permissions(),
    );
    assert_eq!(mode & 0o777, 0o755);
}

#[test]
fn a_config_of_the_persons_own_is_never_overwritten_and_a_second_run_changes_nothing() {
    if !siblings_present() {
        return;
    }
    let jail = Jail::new();
    let mine = jail.config().join("quire/inferd.toml");
    std::fs::create_dir_all(mine.parent().expect("dir")).expect("dir");
    std::fs::write(&mine, "# mine\n[ai]\nlocal_only = \"off\"\n").expect("mine");
    jail.ok(&[]);
    assert_eq!(
        jail.read(".config/quire/inferd.toml"),
        "# mine\n[ai]\nlocal_only = \"off\"\n"
    );
    let first = jail.tree();
    let again = jail.ok(&[]);
    assert_eq!(jail.tree(), first, "idempotent");
    assert!(again.contains("kept (yours)"), "{again}");
    assert_eq!(
        jail.read(".config/quire/inferd.toml"),
        "# mine\n[ai]\nlocal_only = \"off\"\n"
    );
}

#[test]
fn a_dry_run_prints_every_action_and_writes_nothing() {
    if !siblings_present() {
        return;
    }
    let jail = Jail::new();
    let out = jail.ok(&["--dry-run"]);
    assert!(jail.tree().is_empty(), "{:#?}", jail.tree());
    for expect in [
        "install -m 755",
        "intentd",
        "dbus-1/services/org.quire.Companion1.service",
        "quire/skills/desktop-basics/SKILL.md",
        "porter/providers/openrouter.toml",
        "sudo install -D -m644",
    ] {
        assert!(out.contains(expect), "{expect}\n{out}");
    }
    jail.ok(&[]);
    let before = jail.tree();
    let removal = jail.ok(&["--uninstall", "--dry-run"]);
    assert_eq!(jail.tree(), before, "a dry uninstall removes nothing");
    assert!(removal.contains("remove "), "{removal}");
}

#[test]
fn an_uninstall_removes_exactly_what_was_installed_and_keeps_what_is_the_persons() {
    if !siblings_present() {
        return;
    }
    let jail = Jail::new();
    // Something of the person's in a directory the install also uses.
    let theirs = jail.prefix().join("bin/their-tool");
    std::fs::create_dir_all(theirs.parent().expect("dir")).expect("dir");
    std::fs::write(&theirs, "mine").expect("tool");
    let before = jail.tree();
    jail.ok(&[]);
    assert_ne!(jail.tree(), before);
    // The runbook edits inferd.toml: that edit survives.
    let inferd = jail.config().join("quire/inferd.toml");
    let edited = format!(
        "{}\n[ai]\nlocal_only = \"off\"\n",
        std::fs::read_to_string(&inferd).expect("read")
    );
    std::fs::write(&inferd, &edited).expect("edit");
    let out = jail.ok(&["--uninstall"]);
    assert!(out.contains("kept (you edited it)"), "{out}");
    let left = jail.tree();
    let expected: BTreeSet<String> = before
        .into_iter()
        .chain([
            ".config/".to_owned(),
            ".config/quire/".to_owned(),
            ".config/quire/inferd.toml".to_owned(),
        ])
        .collect();
    assert_eq!(left, expected, "only the person's own files remain");
    assert_eq!(std::fs::read_to_string(&inferd).expect("read"), edited);
    assert_eq!(std::fs::read_to_string(&theirs).expect("read"), "mine");
    // Uninstalling again is a no-op.
    let again = jail.ok(&["--uninstall"]);
    assert!(again.contains("nothing to uninstall"), "{again}");
}

#[test]
fn a_missing_binary_stops_the_install_before_anything_is_written() {
    let jail = Jail::new();
    std::fs::remove_file(jail.dir.path().join("fakes/memoryd")).expect("remove");
    let out = jail.run(&[]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("missing: "),
        "{out:?}"
    );
    assert!(jail.tree().is_empty(), "{:#?}", jail.tree());
}

#[test]
fn the_script_never_calls_sudo_or_names_the_real_system() {
    let text = std::fs::read_to_string(Jail::new().script()).expect("script");
    for (n, line) in text.lines().enumerate() {
        let code = line.trim_start();
        // The one `sudo` is the line the script prints, indented inside its here-document.
        assert!(
            !line.starts_with("sudo ") && !code.starts_with("$ sudo"),
            "line {}: {line}",
            n + 1
        );
    }
    assert!(text.contains("set -euo pipefail"));
}

#[test]
fn cloud_is_opt_in_and_an_uninstall_removes_it() {
    if !siblings_present() {
        return;
    }
    let jail = Jail::new();
    jail.ok(&[]);
    let offline = jail.tree();
    // The packaged unit stays as shipped: it has no network.
    assert!(
        jail.read(".config/systemd/user/inferd.service")
            .contains("PrivateNetwork=yes")
    );
    let dry = jail.ok(&["--cloud", "--dry-run"]);
    assert!(dry.contains("inferd.service.d/cloud.conf"), "{dry}");
    assert_eq!(jail.tree(), offline, "a dry run writes nothing");
    jail.ok(&["--cloud"]);
    let dropin = jail.read(".config/systemd/user/inferd.service.d/cloud.conf");
    assert!(dropin.contains("PrivateNetwork=no"), "{dropin}");
    assert!(dropin.contains("AF_INET AF_INET6"), "{dropin}");
    jail.ok(&["--uninstall"]);
    assert!(jail.tree().is_empty(), "{:#?}", jail.tree());
}
