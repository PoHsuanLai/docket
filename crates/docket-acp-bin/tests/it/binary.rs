//! The real `docket-acp` binary, with HOME and every XDG directory scratch: it refuses to serve
//! while `agent.acp.expose` is off, and says why on stderr. Nothing of the real session is read.

use std::process::{Command, Output};

fn run(settings: Option<&str>) -> Output {
    let home = tempfile::tempdir().expect("scratch");
    if let Some(text) = settings {
        let dir = home.path().join("config/docket");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("settings.toml"), text).expect("settings");
    }
    Command::new(env!("CARGO_BIN_EXE_docket-acp"))
        .env_clear()
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_CONFIG_DIRS", home.path().join("none"))
        // A bus that is not there: the process must never find the real session bus.
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path={}", home.path().join("no-bus").display()),
        )
        .output()
        .expect("run")
}

#[test]
fn it_refuses_to_serve_while_the_setting_is_off_or_missing() {
    for settings in [
        None,
        Some("[agent.acp]\nexpose = \"off\"\n"),
        Some("[agent.mcp]\nexpose = \"on\"\n"),
    ] {
        let out = run(settings);
        assert_eq!(out.status.code(), Some(2), "{settings:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("agent.acp.expose is off"), "{err}");
        assert!(
            out.stdout.is_empty(),
            "nothing is written to the protocol stream"
        );
    }
}

#[test]
fn switched_on_it_serves_through_the_bus_and_stops_when_there_is_none() {
    let out = run(Some("[agent.acp]\nexpose = \"on\"\n"));
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("bus"));
}
