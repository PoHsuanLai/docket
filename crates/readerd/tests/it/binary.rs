//! The real `readerd` binary on a private bus: it claims `org.quire.Reader1`, answers only the
//! connection that owns intentd's name, and says "unavailable" (never a value) when the router it
//! is to resolve handles through is not serving. Nothing of the real session is named: the bus,
//! HOME and every XDG directory are scratch.

use crate::support::bus::PrivateBus;
use docket_core::{Handle, ReaderAsk, ReaderError, ReaderTask, Value, ValueSchema};
use docket_dbus::{Details, ReaderProxy};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use zbus::fdo::DBusProxy;

/// A child that is killed when the test ends, however it ends.
struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn readerd(dir: &Path, bus: &str) -> Daemon {
    let home = dir.display().to_string();
    Daemon(
        Command::new(env!("CARGO_BIN_EXE_readerd"))
            .env_clear()
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", dir.join("config"))
            .env("XDG_CONFIG_DIRS", dir.join("none"))
            .env("DBUS_SESSION_BUS_ADDRESS", bus)
            .stdin(Stdio::null())
            .spawn()
            .expect("readerd starts"),
    )
}

async fn has_owner(connection: &zbus::Connection, name: &str) -> bool {
    let dbus = DBusProxy::new(connection).await.expect("dbus");
    let name = zbus::names::BusName::try_from(name).expect("name");
    dbus.name_has_owner(name).await.unwrap_or(false)
}

async fn until_served(connection: &zbus::Connection) {
    for _ in 0..200 {
        if has_owner(connection, "org.quire.Reader1").await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("readerd never claimed org.quire.Reader1");
}

fn ask() -> String {
    serde_json::to_string(&ReaderAsk {
        inputs: vec![Handle(1)],
        want: ValueSchema::Integer { min: 0, max: 9 },
        task: ReaderTask::Extract,
    })
    .expect("json")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_binary_serves_reader1_to_intentd_alone_and_is_unavailable_without_a_router() {
    let dir = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(dir.path());
    let me = bus.connect().await;
    let mut daemon = readerd(dir.path(), bus.address());
    until_served(&me).await;

    // Anyone else is refused before the ask is read.
    let stranger = ReaderProxy::new(&me).await.expect("proxy");
    let refused = stranger
        .extract("s-1", &ask(), &Details::new())
        .await
        .expect_err("a stranger is not answered");
    assert!(refused.to_string().contains("only intentd"), "{refused}");

    // The owner of intentd's name is answered; with no router serving behind that name the handles
    // cannot be resolved, and the reader says so: never a value.
    let intentd = bus.connect().await;
    // A router that answers every member with an error: it owns the name and has nothing to give.
    intentd
        .object_server()
        .at(docket_dbus::INTENTS_PATH, docket_dbus::SessionSkeleton)
        .await
        .expect("a stand-in session object");
    intentd
        .request_name("org.quire.Intents1")
        .await
        .expect("intentd's name");
    let proxy = ReaderProxy::new(&intentd).await.expect("proxy");
    let answer = proxy
        .extract("s-1", &ask(), &Details::new())
        .await
        .expect("an answer");
    let answer: Result<Value, ReaderError> = serde_json::from_str(&answer).expect("json");
    assert_eq!(answer, Err(ReaderError::ModelUnavailable));

    // A second readerd finds the name taken and stops, leaving the first serving.
    let mut second = readerd(dir.path(), bus.address());
    let status = tokio::task::spawn_blocking(move || second.0.wait())
        .await
        .expect("joined")
        .expect("waited");
    assert!(
        !status.success(),
        "a second readerd is not served: {status:?}"
    );
    assert!(has_owner(&me, "org.quire.Reader1").await);

    // Stopping it frees the name.
    daemon.0.kill().expect("kill");
    daemon.0.wait().expect("wait");
    for _ in 0..100 {
        if !has_owner(&me, "org.quire.Reader1").await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the name outlived the daemon");
}
