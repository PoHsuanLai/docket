//! The real `intentd` binary on a private bus: it claims `org.quire.Intents1`, serves the
//! manifests of the scratch data directory, takes its roles from the shipped configuration, and
//! is started by D-Bus activation from an activation file shaped like `dist/dbus/`'s. Nothing
//! of the real session is named: the bus, HOME and every XDG directory are scratch.

use crate::support::bus::PrivateBus;
use docket_client::Transport;
use docket_core::{IntentsReply, IntentsRequest, KillSwitch, WireRefusal};
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
async fn the_binary_serves_the_scratch_manifests_to_the_roles_the_shipped_configuration_names() {
    let dir = tempfile::tempdir().expect("scratch");
    let (_data, env) = scratch_world(dir.path());
    let bus = PrivateBus::start(dir.path());
    let me = bus.connect().await;
    let mut daemon = intentd(&env, bus.address());
    until_served(&me).await;

    // A connection that owns no name is nobody unless its cgroup names it; a plain app owns
    // its name and may read the registry: the scratch manifest, and nothing from the real world.
    let app = bus.connect().await;
    app.request_name("org.quire.Mail").await.expect("name");
    let transport = docket_client::DbusTransport::new(app);
    let IntentsReply::Manifests(all) = transport
        .call(IntentsRequest::Manifests)
        .await
        .expect("manifests")
    else {
        panic!("manifests");
    };
    let names: Vec<String> = all.iter().map(|m| m.manifest().app.to_string()).collect();
    assert_eq!(
        names,
        ["org.quire.Companion", "org.quire.Mail", "org.quire.Memory"],
        "the data dir's one app, and the two providers intentd hosts itself"
    );

    // sill's name plays the control centre; a plain app does not.
    let sill = bus.connect().await;
    sill.request_name("org.quire.Shell").await.expect("name");
    let control = docket_client::DbusTransport::new(sill);
    let state = control
        .call(IntentsRequest::ControlState)
        .await
        .expect("state");
    assert!(
        matches!(state, IntentsReply::State(KillSwitch { .. })),
        "{state:?}"
    );
    let refused = transport
        .call(IntentsRequest::ControlState)
        .await
        .expect("a reply");
    assert_eq!(refused, IntentsReply::Refused(WireRefusal::NotAllowed));

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

/// The line intentd says about `INTENTD_PROC_ROOT` when it serves with it set to `fixture`.
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

#[cfg(not(feature = "test-proc-root"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_default_build_ignores_the_proc_root_variable_and_says_so() {
    let fixture = tempfile::tempdir().expect("fixture");
    let said = said_about_the_proc_root(fixture.path()).await;
    assert!(
        said.contains("INTENTD_PROC_ROOT is set but this build has no test-proc-root feature"),
        "{said}"
    );
    assert!(!said.contains("TEST BUILD"), "{said}");
}

#[cfg(feature = "test-proc-root")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_test_build_honours_the_proc_root_variable_and_names_the_root() {
    let fixture = tempfile::tempdir().expect("fixture");
    let said = said_about_the_proc_root(fixture.path()).await;
    let root = fixture.path().display().to_string();
    assert!(
        said.contains(&format!(
            "TEST BUILD: reading callers from the proc root {root}"
        )),
        "{said}"
    );
}
