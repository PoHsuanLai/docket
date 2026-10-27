//! The ACP engine's login: staged mode 0600, removed when the guard goes, and scrubbed from what the
//! run writes.

use docket_live::acp::{Credentials, CredentialsSource, Redactor};

const SECRET: &str = "sk-ant-oat01-SECRET-TOKEN-0123456789";
const REFRESH: &str = "sk-ant-ort01-REFRESH-TOKEN-abcdefghij";

fn login_json() -> String {
    format!(
        r#"{{"claudeAiOauth":{{"accessToken":"{SECRET}","refreshToken":"{REFRESH}","expiresAt":1,"scopes":["user:inference"],"subscriptionType":"max"}}}}"#
    )
}

fn source_in(dir: &std::path::Path) -> CredentialsSource {
    let from = dir.join("source.json");
    std::fs::write(&from, login_json()).expect("source");
    CredentialsSource {
        from,
        at: ".claude/.credentials.json".to_owned(),
    }
}

#[test]
fn the_login_is_copied_mode_0600_and_removed_when_the_guard_goes() {
    let dir = tempfile::tempdir().expect("scratch");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let source = source_in(dir.path());
    let at;
    {
        let (guard, _) = Credentials::stage(&source, &home).expect("staged");
        at = guard.path().to_owned();
        assert_eq!(at, home.join(".claude/.credentials.json"));
        assert_eq!(std::fs::read_to_string(&at).expect("copy"), login_json());
        let mode = std::fs::metadata(&at).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let dir_mode = std::fs::metadata(at.parent().expect("dir"))
            .expect("meta")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
    }
    assert!(!at.exists(), "dropped, so gone");
    // The source is untouched.
    assert_eq!(
        std::fs::read_to_string(&source.from).expect("source"),
        login_json()
    );
}

#[test]
fn a_panic_removes_the_login_too() {
    let dir = tempfile::tempdir().expect("scratch");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let source = source_in(dir.path());
    let target = home.join(".claude/.credentials.json");
    let ran = std::panic::catch_unwind(|| {
        let (_guard, _) = Credentials::stage(&source, &home).expect("staged");
        assert!(target.exists());
        panic!("the run fails half way");
    });
    assert!(ran.is_err());
    assert!(!target.exists(), "the login does not outlive a panic");
}

#[test]
fn a_missing_source_stages_nothing_and_names_only_the_path() {
    let dir = tempfile::tempdir().expect("scratch");
    let source = CredentialsSource {
        from: dir.path().join("nope.json"),
        at: ".claude/.credentials.json".to_owned(),
    };
    let fault = Credentials::stage(&source, dir.path()).expect_err("no source");
    assert!(!fault.to_string().contains("SECRET"));
    assert!(!dir.path().join(".claude").exists());
}

#[test]
fn the_secret_and_every_token_in_it_are_scrubbed_but_short_words_are_not() {
    let redactor = Redactor::of(login_json().as_bytes());
    let text = format!(
        "said {SECRET} and {REFRESH} and {} on a plan of max",
        login_json()
    );
    let clean = redactor.scrub(&text);
    assert!(!clean.contains("SECRET-TOKEN"), "{clean}");
    assert!(!clean.contains("REFRESH-TOKEN"), "{clean}");
    assert!(clean.contains("a plan of max"), "{clean}");
    assert!(redactor.is_clean(clean.as_bytes()));
    assert!(!redactor.is_clean(text.as_bytes()));
    assert!(!format!("{redactor:?}").contains("SECRET"));
    // Nothing to scrub, nothing changed.
    assert_eq!(Redactor::none().scrub("anything"), "anything");
}

#[test]
fn a_sweep_rewrites_the_files_that_hold_the_secret_and_leaves_the_others() {
    let dir = tempfile::tempdir().expect("scratch");
    std::fs::create_dir_all(dir.path().join("logs")).expect("dirs");
    let dirty = dir.path().join("logs/intentd.log");
    let fine = dir.path().join("logs/other.log");
    std::fs::write(&dirty, format!("call args {SECRET}\n")).expect("dirty");
    std::fs::write(&fine, "all quiet\n").expect("fine");
    Redactor::of(login_json().as_bytes()).sweep(dir.path());
    assert_eq!(
        std::fs::read_to_string(&dirty).expect("dirty"),
        "call args [redacted]\n"
    );
    assert_eq!(std::fs::read_to_string(&fine).expect("fine"), "all quiet\n");
}
