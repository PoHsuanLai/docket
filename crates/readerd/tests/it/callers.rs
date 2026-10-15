//! Who readerd answers, over a private bus and a fake proc root: the process in intentd's unit,
//! as the shipped callers file names it, and no other. A scope that claims the app, a process of
//! the unit that is not its main process, a unit the table does not name and a process nothing
//! names are refused before the ask is read. The test process is every connection, so the fake
//! `/proc/<pid>/cgroup` says what it is.

use crate::served::service;
use crate::support::bus::PrivateBus;
use docket_core::{Handle, ReaderAsk, ReaderError, ReaderTask, Value, ValueSchema};
use docket_dbus::callers::Identity;
use docket_dbus::{Details, ReaderProxy};
use readerd::serve_on;
use std::path::Path;
use std::sync::Arc;

/// Whether the test process is the main process of the unit it runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Main {
    Yes,
    No,
}

/// This process in the fake proc root `root`, in the cgroup `leaf`.
fn place_me(root: &Path, leaf: &str, main: Main) {
    let pid = std::process::id();
    let dir = root.join(pid.to_string());
    std::fs::create_dir_all(&dir).expect("pid dir");
    let slice = "0::/user.slice/user-1000.slice/user@1000.service/app.slice";
    std::fs::write(dir.join("cgroup"), format!("{slice}/{leaf}\n")).expect("cgroup");
    if main == Main::Yes {
        std::fs::create_dir_all(root.join("units")).expect("units dir");
        std::fs::write(root.join("units").join(leaf), pid.to_string()).expect("main pid");
    }
}

fn ask() -> String {
    serde_json::to_string(&ReaderAsk {
        inputs: vec![Handle(1)],
        want: ValueSchema::Integer { min: 0, max: 9 },
        task: ReaderTask::Extract,
    })
    .expect("json")
}

/// What readerd answers this process when it is in `leaf` (`None`: no cgroup file at all).
async fn asked_in(leaf: Option<(&str, Main)>) -> Result<String, zbus::Error> {
    let scratch = tempfile::tempdir().expect("scratch");
    let root = scratch.path().join("proc");
    std::fs::create_dir_all(&root).expect("proc root");
    if let Some((leaf, main)) = leaf {
        place_me(&root, leaf, main);
    }
    let bus = PrivateBus::start(scratch.path());
    let daemon = bus.connect().await;
    let who = Arc::new(Identity::on_fixture(
        daemon.clone(),
        readerd::shipped_callers(),
        root,
    ));
    serve_on(&daemon, service(), who).await.expect("serves");
    let me = bus.connect().await;
    let proxy = ReaderProxy::new(&me).await.expect("proxy");
    proxy.extract("s-1", &ask(), &Details::new()).await
}

fn denied(error: &zbus::Error) -> bool {
    matches!(error, zbus::Error::MethodError(name, _, _)
        if name.as_str() == "org.freedesktop.DBus.Error.AccessDenied")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_the_main_process_of_intentds_unit_is_answered() {
    // intentd's unit is answered: the ask is read, and with no model and no session to read from
    // there is no value, never an AccessDenied.
    let answer = asked_in(Some(("intentd.service", Main::Yes)))
        .await
        .expect("an answer");
    let answer: Result<Value, ReaderError> = serde_json::from_str(&answer).expect("json");
    assert!(answer.is_err(), "{answer:?}");
    for (what, leaf) in [
        (
            "a process of intentd's unit that is not its main process",
            Some(("intentd.service", Main::No)),
        ),
        (
            "an app scope called like intentd",
            Some(("app-org.quire.Intents-1.scope", Main::No)),
        ),
        (
            "a scope that claims intentd's name",
            Some(("intentd.scope", Main::No)),
        ),
        (
            "a unit the table does not name",
            Some(("companiond.service", Main::Yes)),
        ),
        ("a terminal's child", Some(("vte-spawn-1.scope", Main::No))),
        ("a process with no cgroup", None),
    ] {
        let refused = asked_in(leaf).await.expect_err(what);
        assert!(denied(&refused), "{what}: {refused}");
    }
}
