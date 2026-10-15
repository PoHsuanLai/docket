//! The real `intentd` binary on a private bus: it claims `org.quire.Intents1`, gives no role to a
//! mere name, and is started by D-Bus activation from an activation file shaped like `dist/dbus/`'s. Nothing
//! of the real session is named: the bus, HOME and every XDG directory are scratch.

use crate::support::bus::PrivateBus;
use docket_client::Transport;
use docket_core::{IntentsReply, IntentsRequest, WireRefusal};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use zbus::fdo::DBusProxy;

fn scratch_world(dir: &Path) -> (PathBuf, Vec<(String, String)>) {
    let data = dir.join("data");
    let intents = data.join("quire/intents");
    std::fs::create_dir_all(&intents).expect("dirs");
    std::fs::write(
        intents.join("org.quire.Mail.toml"),
        docket_fake::MAIL_MANIFEST,
    )
    .expect("manifest");
    let home = dir.display().to_string();
    let env = [
        ("HOME", home.clone()),
        ("XDG_DATA_HOME", data.display().to_string()),
        ("XDG_DATA_DIRS", dir.join("none").display().to_string()),
        ("XDG_CONFIG_HOME", dir.join("config").display().to_string()),
        ("XDG_CONFIG_DIRS", dir.join("none").display().to_string()),
    ];
    (
        data,
        env.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
    )
}

/// A child that is killed when the test ends, however it ends.
struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn intentd(env: &[(String, String)], bus: &str) -> Daemon {
    Daemon(
        Command::new(env!("CARGO_BIN_EXE_intentd"))
            .env_clear()
            .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .env("DBUS_SESSION_BUS_ADDRESS", bus)
            .env("DBUS_SYSTEM_BUS_ADDRESS", bus)
            .stdin(Stdio::null())
            .spawn()
            .expect("intentd starts"),
    )
}

async fn has_owner(connection: &zbus::Connection) -> bool {
    let dbus = DBusProxy::new(connection).await.expect("dbus");
    let name = zbus::names::BusName::try_from("org.quire.Intents1").expect("name");
    dbus.name_has_owner(name).await.unwrap_or(false)
}

async fn until_served(connection: &zbus::Connection) {
    for _ in 0..200 {
        if has_owner(connection).await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("intentd never claimed org.quire.Intents1");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_binary_claims_the_name_and_a_name_alone_gives_no_role() {
    let dir = tempfile::tempdir().expect("scratch");
    let (_data, env) = scratch_world(dir.path());
    let bus = PrivateBus::start(dir.path());
    let me = bus.connect().await;
    let mut daemon = intentd(&env, bus.address());
    until_served(&me).await;

    // The binary reads the system's `/proc` and the callers file under `/etc/quire`, which no
    // test may move: which role the process behind a connection has is tested in process, over a
    // fake proc root (`identity.rs`, `bus_members.rs`). What holds whatever this test process is:
    // owning sill's name makes it neither sill nor the control centre, and the same for a plain
    // app's. Only the shell's unit, as the system's table names it, is the control centre.
    for name in ["org.quire.Shell", "org.quire.Mail"] {
        let connection = bus.connect().await;
        connection.request_name(name).await.expect("name");
        let refused = docket_client::DbusTransport::new(connection)
            .call(IntentsRequest::ControlState)
            .await
            .expect("a reply");
        assert_eq!(
            refused,
            IntentsReply::Refused(WireRefusal::NotAllowed),
            "{name}"
        );
    }

    // Stopping it frees the name.
    daemon.0.kill().expect("kill");
    daemon.0.wait().expect("wait");
    for _ in 0..100 {
        if !has_owner(&me).await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the name outlived the daemon");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn d_bus_activation_starts_the_binary_for_a_caller_and_a_second_intentd_cannot_take_the_name()
{
    let dir = tempfile::tempdir().expect("scratch");
    let (_data, env) = scratch_world(dir.path());
    // The activation file is `dist/dbus/org.quire.Intents1.service` with this binary as Exec.
    let services = dir.path().join("services");
    std::fs::create_dir_all(&services).expect("dir");
    let shipped = include_str!("../../../../dist/dbus/org.quire.Intents1.service");
    assert!(shipped.contains("Name=org.quire.Intents1"));
    std::fs::write(
        services.join("org.quire.Intents1.service"),
        format!(
            "[D-BUS Service]\nName=org.quire.Intents1\nExec={}\n",
            env!("CARGO_BIN_EXE_intentd")
        ),
    )
    .expect("service file");
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let bus = PrivateBus::start_with(dir.path(), Some(&services), &env_refs);
    let me = bus.connect().await;
    assert!(!has_owner(&me).await, "not running yet");

    // What `DbusTransport::connect` does: ask the bus to start it.
    let dbus = DBusProxy::new(&me).await.expect("dbus");
    let name = zbus::names::BusName::try_from("org.quire.Intents1").expect("name");
    dbus.start_service_by_name(name.clone().try_into().expect("well known"), 0)
        .await
        .expect("activated");
    until_served(&me).await;

    // A second one finds the name taken and stops, leaving the first serving.
    let mut second = intentd(&env, bus.address());
    let status = tokio::task::spawn_blocking(move || second.0.wait())
        .await
        .expect("joined")
        .expect("waited");
    assert!(
        !status.success(),
        "a second intentd is not served: {status:?}"
    );
    assert!(has_owner(&me).await, "the first is still there");
}

/// What intentd says on stderr when it serves with `INTENTD_PROC_ROOT` set to `fixture`.
async fn said_about_the_proc_root(fixture: &Path) -> String {
    let dir = tempfile::tempdir().expect("scratch");
    let (_data, mut env) = scratch_world(dir.path());
    env.push((
        "INTENTD_PROC_ROOT".to_owned(),
        fixture.display().to_string(),
    ));
    let bus = PrivateBus::start(dir.path());
    let me = bus.connect().await;
    let log = dir.path().join("intentd.log");
    let mut daemon = Daemon(
        Command::new(env!("CARGO_BIN_EXE_intentd"))
            .env_clear()
            .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .env("DBUS_SESSION_BUS_ADDRESS", bus.address())
            .env("DBUS_SYSTEM_BUS_ADDRESS", bus.address())
            .stdin(Stdio::null())
            .stderr(std::fs::File::create(&log).expect("log"))
            .spawn()
            .expect("intentd starts"),
    );
    until_served(&me).await;
    daemon.0.kill().expect("kill");
    daemon.0.wait().expect("wait");
    std::fs::read_to_string(&log).expect("the log")
}

/// No environment variable moves the proc root of the shipped daemon: it is silent about it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_shipped_daemon_ignores_the_proc_root_variable_without_a_word() {
    let fixture = tempfile::tempdir().expect("fixture");
    let said = said_about_the_proc_root(fixture.path()).await;
    assert!(!said.contains("proc root"), "{said}");
    assert!(!said.contains("INTENTD_PROC_ROOT"), "{said}");
}
