//! The real `companiond` binary on a private bus: it claims `org.quire.Companion1`, answers
//! `Roster()` and `Front()` with nothing running, takes the name of the shell from its
//! configuration, and speaks for the person to no one else. With no router on the bus it still
//! serves: a restart that finds nothing resumes nothing. Nothing of the real session is named: the
//! bus, HOME and every XDG directory are scratch.

mod support;

use companion_wire::FrontTask;
use docket_core::{Roster, TurnId, TurnSource, TurnVia, UserTurn};
use docket_dbus::CompanionProxy;
use prov::{AgentRef, TaskId};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use support::bus::PrivateBus;
use zbus::fdo::DBusProxy;

/// A child that is killed when the test ends, however it ends.
struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn companiond(dir: &Path, bus: &str) -> Daemon {
    let home = dir.display().to_string();
    Daemon(
        Command::new(env!("CARGO_BIN_EXE_companiond"))
            .env_clear()
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", dir.join("config"))
            .env("XDG_CONFIG_DIRS", dir.join("none"))
            .env("DBUS_SESSION_BUS_ADDRESS", bus)
            .stdin(Stdio::null())
            .spawn()
            .expect("companiond starts"),
    )
}

async fn has_owner(connection: &zbus::Connection, name: &str) -> bool {
    let dbus = DBusProxy::new(connection).await.expect("dbus");
    let name = zbus::names::BusName::try_from(name).expect("name");
    dbus.name_has_owner(name).await.unwrap_or(false)
}

async fn until_served(connection: &zbus::Connection) {
    for _ in 0..200 {
        if has_owner(connection, "org.quire.Companion1").await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("companiond never claimed org.quire.Companion1");
}

fn turn() -> String {
    serde_json::to_string(&UserTurn {
        id: TurnId(1),
        text: "skip the newsletters".into(),
        at: prov::UnixSeconds(5),
        from: TurnSource::Launcher,
        via: TurnVia::Typed,
    })
    .expect("json")
}

fn worker() -> String {
    serde_json::to_string(&AgentRef::Worker {
        task: TaskId::parse("t-9").expect("task"),
    })
    .expect("json")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_binary_serves_companion1_for_the_shell_its_configuration_names() {
    let dir = tempfile::tempdir().expect("scratch");
    // The configuration names another shell than the shipped one.
    let config = dir.path().join("config/quire");
    std::fs::create_dir_all(&config).expect("dirs");
    std::fs::write(
        config.join("companiond.toml"),
        "shell = \"org.quire.Desk\"\n",
    )
    .expect("config");
    let bus = PrivateBus::start(dir.path());
    let me = bus.connect().await;
    let mut daemon = companiond(dir.path(), bus.address());
    until_served(&me).await;

    // Nothing is running, and the roster and the front task say so without any router.
    let proxy = CompanionProxy::new(&me).await.expect("proxy");
    let roster: Roster =
        serde_json::from_str(&proxy.roster().await.expect("roster")).expect("json");
    assert!(roster.entries.is_empty());
    let front: FrontTask =
        serde_json::from_str(&proxy.front().await.expect("front")).expect("json");
    assert_eq!(front.task, None);

    // Only the configured shell speaks for the person.
    assert!(
        proxy.told(&worker(), "work", &turn()).await.is_err(),
        "a stranger is refused"
    );
    let shipped_shell = bus.connect().await;
    shipped_shell
        .request_name("org.quire.Shell")
        .await
        .expect("name");
    assert!(
        CompanionProxy::new(&shipped_shell)
            .await
            .expect("proxy")
            .told(&worker(), "work", &turn())
            .await
            .is_err(),
        "the shipped name is not the configured one"
    );
    let desk = bus.connect().await;
    desk.request_name("org.quire.Desk").await.expect("name");
    CompanionProxy::new(&desk)
        .await
        .expect("proxy")
        .told(&worker(), "work", &turn())
        .await
        .expect("the configured shell is heard");

    // A second companiond finds the name taken and stops, leaving the first serving.
    let mut second = companiond(dir.path(), bus.address());
    let status = tokio::task::spawn_blocking(move || second.0.wait())
        .await
        .expect("joined")
        .expect("waited");
    assert!(
        !status.success(),
        "a second companiond is not served: {status:?}"
    );
    assert!(has_owner(&me, "org.quire.Companion1").await);

    // Stopping it frees the name.
    daemon.0.kill().expect("kill");
    daemon.0.wait().expect("wait");
    for _ in 0..100 {
        if !has_owner(&me, "org.quire.Companion1").await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the name outlived the daemon");
}

#[test]
fn the_shipped_configuration_is_the_defaults_and_a_bad_file_is_refused() {
    use companiond::CompaniondConfig;
    assert_eq!(
        CompaniondConfig::shipped().expect("shipped"),
        CompaniondConfig::default()
    );
    assert!(CompaniondConfig::parse("shell = 4").is_err());
    assert!(CompaniondConfig::parse("shel = \"org.quire.Desk\"").is_err());
}
