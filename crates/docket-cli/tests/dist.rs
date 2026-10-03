//! The completion files under `dist/completions` are thin: each hands what is typed to
//! `quire-do __complete` and offers what comes back. A shell that is installed checks the
//! syntax; bash also runs the function against a stand-in `quire-do`.

use std::path::PathBuf;
use std::process::Command;

fn dist(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../dist/completions")
        .join(name)
}

fn read(name: &str) -> String {
    std::fs::read_to_string(dist(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn has(shell: &str) -> bool {
    Command::new(shell).arg("--version").output().is_ok()
}

#[test]
fn the_three_files_ask_quire_do_what_to_offer() {
    for name in ["quire-do.bash", "_quire-do", "quire-do.fish"] {
        let text = read(name);
        assert!(text.contains("quire-do __complete"), "{name}");
        assert!(
            !text.contains("--yes"),
            "{name}: nothing here offers a way around the gate"
        );
    }
    assert!(read("_quire-do").starts_with("#compdef quire-do"));
}

#[test]
fn installed_shells_accept_the_syntax() {
    for (shell, file, flag) in [
        ("bash", "quire-do.bash", "-n"),
        ("zsh", "_quire-do", "-n"),
        ("fish", "quire-do.fish", "--no-execute"),
    ] {
        if !has(shell) {
            println!("{shell} is not installed here; its file is not syntax-checked");
            continue;
        }
        let out = Command::new(shell)
            .arg(flag)
            .arg(dist(file))
            .output()
            .expect("shell runs");
        assert!(
            out.status.success(),
            "{shell} rejects {file}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn bash_hands_the_typed_words_to_quire_do() {
    if !has("bash") {
        return;
    }
    let script = format!(
        r#"
quire-do() {{ printf '%s\n' "$@"; }}
source {}
COMP_WORDS=(quire-do mail thread.archive --)
COMP_CWORD=3
_quire_do
printf '%s|' "${{COMPREPLY[@]}}"
"#,
        dist("quire-do.bash").display()
    );
    let out = Command::new("bash")
        .arg("-c")
        .arg(script)
        .output()
        .expect("bash runs");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "__complete|mail|thread.archive|--|"
    );
}
