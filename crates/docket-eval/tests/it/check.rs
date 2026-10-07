//! The conformance check over app repositories built in a scratch directory: a `.desktop` file
//! with no manifest fails, a manifest that is not valid fails, a menu command or shortcut with no
//! action fails unless it is UI-only with a reason; and the binary and `scripts/check-intents.sh`
//! say so with their exit codes.

use docket_eval::{Level, check_app};
use docket_fake::MAIL_MANIFEST;
use std::path::{Path, PathBuf};
use std::process::Command;

fn put(root: &Path, path: &str, text: &str) {
    let full = root.join(path);
    std::fs::create_dir_all(full.parent().expect("parent")).expect("dirs");
    std::fs::write(full, text).expect("write");
}

const DESKTOP: &str = "[Desktop Entry]\nType=Application\nName=Mail\nExec=mailo\n";

fn failures(root: &Path) -> Vec<String> {
    check_app(root)
        .expect("readable")
        .findings
        .into_iter()
        .filter(|f| f.level == Level::Fail)
        .map(|f| format!("{}: {}", f.file.display(), f.what))
        .collect()
}

#[test]
fn a_desktop_file_with_no_manifest_fails() {
    let dir = tempfile::tempdir().expect("dir");
    put(dir.path(), "dist/mailo.desktop", DESKTOP);
    let found = failures(dir.path());
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("dist/mailo.desktop"), "{found:?}");
    assert!(found[0].contains("no intents manifest"), "{found:?}");
}

#[test]
fn a_desktop_file_with_its_manifest_passes() {
    let dir = tempfile::tempdir().expect("dir");
    put(dir.path(), "dist/mailo.desktop", DESKTOP);
    put(
        dir.path(),
        "dist/intents/org.quire.Mail.toml",
        MAIL_MANIFEST,
    );
    assert_eq!(failures(dir.path()), Vec::<String>::new());
    // The shipped built-in manifests are manifests too.
    put(
        dir.path(),
        "dist/intents/org.quire.Memory.toml",
        include_str!("../../../../manifests/org.quire.Memory.toml"),
    );
    assert_eq!(failures(dir.path()), Vec::<String>::new());
}

#[test]
fn a_reverse_dns_desktop_file_needs_its_own_apps_manifest() {
    let dir = tempfile::tempdir().expect("dir");
    put(dir.path(), "dist/org.quire.Detent.desktop", DESKTOP);
    put(
        dir.path(),
        "dist/intents/org.quire.Mail.toml",
        MAIL_MANIFEST,
    );
    let found = failures(dir.path());
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("org.quire.Detent"), "{found:?}");
    put(
        dir.path(),
        "dist/intents/org.quire.Detent.toml",
        &MAIL_MANIFEST
            .replace("org.quire.Mail", "org.quire.Detent")
            .replace("mail.", "detent."),
    );
    assert_eq!(failures(dir.path()), Vec::<String>::new());
}

#[test]
fn a_manifest_that_is_not_valid_fails_and_says_why() {
    let cases: [(&str, String, &str); 4] = [
        (
            "an action with no parameter schema",
            MAIL_MANIFEST
                .replacen("[[actions.params]]", "[[actions.not_params]]", 1)
                .replace("params = []", ""),
            "not a valid manifest",
        ),
        (
            "a write with no undo that is not destructive",
            MAIL_MANIFEST
                .replacen(
                    "effect = \"undoable_write\"",
                    "effect = \"undoable_write\"\n# x",
                    1,
                )
                .replace("undo = \"token\"", "undo = \"not_undoable\""),
            "writes without undo",
        ),
        (
            "a vocabulary from the future",
            MAIL_MANIFEST.replacen("vocab = 1", "vocab = 99", 1),
            "newer than this build",
        ),
        (
            "not TOML at all",
            "this is not a manifest".to_owned(),
            "not a valid manifest",
        ),
    ];
    for (name, text, why) in cases {
        let dir = tempfile::tempdir().expect("dir");
        put(dir.path(), "dist/mailo.desktop", DESKTOP);
        put(dir.path(), "dist/intents/org.quire.Mail.toml", &text);
        let found = failures(dir.path());
        assert!(found.iter().any(|f| f.contains(why)), "{name}: {found:?}");
        assert!(
            found.iter().any(|f| f.starts_with("dist/mailo.desktop")),
            "{name}: an invalid manifest is not a manifest: {found:?}"
        );
    }
}

#[test]
fn fixtures_build_output_links_and_hidden_directories_are_not_the_app() {
    let dir = tempfile::tempdir().expect("dir");
    put(dir.path(), "tests/fixtures/a.desktop", DESKTOP);
    put(
        dir.path(),
        "crates/x/tests/fixtures/applications/b.desktop",
        DESKTOP,
    );
    put(dir.path(), "target/debug/c.desktop", DESKTOP);
    put(dir.path(), ".git/d.desktop", DESKTOP);
    put(
        dir.path(),
        "dist/links/e.desktop",
        "[Desktop Entry]\nType=Link\nURL=https://example.org\n",
    );
    put(dir.path(), "tests/fixtures/broken.intents.toml", "garbage");
    let report = check_app(dir.path()).expect("readable");
    assert!(report.findings.is_empty(), "{}", report.render());
}

const MAIL_UI: &str = r#"
vocab = 1
app = "org.quire.Mail"

[[commands]]
id = "file.new"
source = "menu"
chord = "cmd+n"
action = "mail.draft.create"

[[commands]]
id = "thread.archive"
source = "shortcut"
chord = "e"
action = "mail.thread.archive"

[[commands]]
id = "window.minimize"
source = "menu"

[[ui_only]]
id = "window.minimize"
reason = "window chrome"
"#;

fn with_ui(ui: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("dir");
    put(dir.path(), "dist/mailo.desktop", DESKTOP);
    put(
        dir.path(),
        "dist/intents/org.quire.Mail.toml",
        MAIL_MANIFEST,
    );
    put(dir.path(), "dist/intents/org.quire.Mail.ui.toml", ui);
    dir
}

#[test]
fn every_menu_command_and_shortcut_is_an_action_or_ui_only_with_a_reason() {
    let dir = with_ui(MAIL_UI);
    assert_eq!(failures(dir.path()), Vec::<String>::new());
    let rows: [(&str, String, &str); 5] = [
        (
            "a command with no action and no ui_only row",
            MAIL_UI.replace(
                "[[ui_only]]\nid = \"window.minimize\"\nreason = \"window chrome\"\n",
                "",
            ),
            "is not listed under ui_only",
        ),
        (
            "a ui_only row with no reason",
            MAIL_UI.replace("window chrome", "  "),
            "no reason",
        ),
        (
            "an action the manifest does not declare",
            MAIL_UI.replace("mail.thread.archive", "mail.thread.explode"),
            "does not declare",
        ),
        (
            "a file for another app",
            MAIL_UI.replace("org.quire.Mail", "org.quire.Files"),
            "not org.quire.Mail",
        ),
        (
            "a file that is not the shape",
            "[[commands]]\nid = 1".to_owned(),
            "not a ui commands file",
        ),
    ];
    for (name, text, why) in rows {
        let dir = with_ui(&text);
        let found = failures(dir.path());
        assert!(found.iter().any(|f| f.contains(why)), "{name}: {found:?}");
    }
}

#[test]
fn without_a_ui_file_the_check_says_what_it_cannot_see_and_does_not_fail() {
    let dir = tempfile::tempdir().expect("dir");
    put(dir.path(), "dist/mailo.desktop", DESKTOP);
    put(
        dir.path(),
        "dist/intents/org.quire.Mail.toml",
        MAIL_MANIFEST,
    );
    let report = check_app(dir.path()).expect("readable");
    assert_eq!(report.failures(), 0, "{}", report.render());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.level == Level::Note && f.what.contains("not checked")),
        "{}",
        report.render()
    );
}

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_docket-eval"))
}

#[test]
fn the_binary_exits_0_1_or_2() {
    let good = tempfile::tempdir().expect("dir");
    put(good.path(), "dist/mailo.desktop", DESKTOP);
    put(
        good.path(),
        "dist/intents/org.quire.Mail.toml",
        MAIL_MANIFEST,
    );
    let bad = tempfile::tempdir().expect("dir");
    put(bad.path(), "dist/mailo.desktop", DESKTOP);
    let run = |dir: &Path| {
        Command::new(binary())
            .arg("--check-app")
            .arg(dir)
            .output()
            .expect("runs")
    };
    let ok = run(good.path());
    assert_eq!(
        ok.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    assert!(String::from_utf8_lossy(&ok.stdout).contains("ok   dist/intents/org.quire.Mail.toml"));
    let fail = run(bad.path());
    assert_eq!(fail.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&fail.stdout).contains("FAIL dist/mailo.desktop"));
    let missing = run(&good.path().join("nowhere"));
    assert_eq!(missing.status.code(), Some(2));
    let usage = Command::new(binary()).output().expect("runs");
    assert_eq!(usage.status.code(), Some(2));
}

#[test]
fn the_script_runs_the_binary_and_passes_its_exit_code_on() {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/check-intents.sh");
    let good = tempfile::tempdir().expect("dir");
    put(good.path(), "dist/mailo.desktop", DESKTOP);
    put(
        good.path(),
        "dist/intents/org.quire.Mail.toml",
        MAIL_MANIFEST,
    );
    let bad = tempfile::tempdir().expect("dir");
    put(bad.path(), "dist/mailo.desktop", DESKTOP);
    let run = |dir: &Path| {
        Command::new("bash")
            .arg(&script)
            .arg(dir)
            .env("DOCKET_EVAL", binary())
            .output()
            .expect("runs")
    };
    assert_eq!(run(good.path()).status.code(), Some(0));
    assert_eq!(run(bad.path()).status.code(), Some(1));
    let nowhere = Command::new("bash")
        .arg(&script)
        .arg(good.path())
        .env_remove("DOCKET_EVAL")
        .env("DOCKET_DIR", good.path().join("no-docket-here"))
        .output()
        .expect("runs");
    assert_eq!(nowhere.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&nowhere.stderr).contains("no docket checkout"));
}

// ---- --check-skills ----

const SHELL_MANIFEST: &str = r#"vocab = 1
app = "org.quire.Shell"
entities = []

[[actions]]
name = "shell.window.hide"
label = "Hide"
on = { kind = "nothing" }
effect = "read"
classes = ["app_own"]
undo = "not_undoable"
reach = "offered"
latency = "quick"
result = { kind = "nothing" }
keys = { kind = "none" }
lasting = "no"
dry_run = "none"
params = []
"#;

const HIDE_MD: &str = "---\nname: hiding\ndescription: Hide a window.\n---\n# Hiding\n\nHide it.\n";

const HIDE_SKILL: &str = "vocab = 1\nid = \"hiding\"\nowner = \"org.quire.Shell\"\nversion = \"0.1.0\"\nuses = [\"org.quire.Shell:shell.window.hide\"]\n[when]\nalways = \"yes\"\n";

fn skill_repo(dir: &Path) {
    put(dir, "dist/skills/hiding/skill.toml", HIDE_SKILL);
    put(dir, "dist/skills/hiding/SKILL.md", HIDE_MD);
}

fn run_skills(args: &[&std::ffi::OsStr]) -> (Option<i32>, String) {
    let out = Command::new(binary())
        .arg("--check-skills")
        .args(args)
        .output()
        .expect("runs");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr),
    )
}

#[test]
fn a_repo_with_dist_intents_and_dist_skills_validates_whichever_directory_is_named() {
    let repo = tempfile::tempdir().expect("dir");
    skill_repo(repo.path());
    // Without its manifest the skill's action is missing.
    let skills = repo.path().join("dist/skills");
    let (code, said) = run_skills(&[skills.as_os_str()]);
    assert_eq!(code, Some(1), "{said}");
    assert!(said.contains("which no manifest declares"), "{said}");
    put(
        repo.path(),
        "dist/intents/org.quire.Shell.toml",
        SHELL_MANIFEST,
    );
    // Named by the repository root, and by its skills directory alone.
    for dir in [repo.path().to_path_buf(), skills] {
        let (code, said) = run_skills(&[dir.as_os_str()]);
        assert_eq!(code, Some(0), "{dir:?}: {said}");
        assert!(said.contains("hiding 0.1.0 validates"), "{said}");
    }
}

#[test]
fn manifests_adds_a_directory_of_manifests_and_may_be_repeated() {
    let repo = tempfile::tempdir().expect("dir");
    put(repo.path(), "skills/hiding/skill.toml", HIDE_SKILL);
    put(repo.path(), "skills/hiding/SKILL.md", HIDE_MD);
    let elsewhere = tempfile::tempdir().expect("dir");
    put(elsewhere.path(), "org.quire.Shell.toml", SHELL_MANIFEST);
    let nothing = tempfile::tempdir().expect("dir");
    let skills = repo.path().join("skills");
    let (code, said) = run_skills(&[skills.as_os_str()]);
    assert_eq!(code, Some(1), "{said}");
    let (code, said) = run_skills(&[
        skills.as_os_str(),
        "--manifests".as_ref(),
        nothing.path().as_os_str(),
        "--manifests".as_ref(),
        elsewhere.path().as_os_str(),
    ]);
    assert_eq!(code, Some(0), "{said}");
    // A flag with no value is a usage error.
    let (code, _) = run_skills(&[skills.as_os_str(), "--manifests".as_ref()]);
    assert_eq!(code, Some(2));
    let (code, _) = run_skills(&["--manifests".as_ref(), elsewhere.path().as_os_str()]);
    assert_eq!(code, Some(2));
}
