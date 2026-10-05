//! `quire-do` end to end: the real binary, intentd's real `start` (the router over `DbusLink`,
//! `SheetConfirmer` and `FileGrants`, serving `org.quire.Intents1`), a mail app that is
//! docket-fake's `FakeMail` behind `docket_client::serve_on`, and sill's `Confirm1` as a fake
//! that answers from a script. All of it on a private `dbus-daemon`, with a scratch HOME and
//! XDG directories (the app's manifest is installed under the scratch `XDG_DATA_DIRS`): the
//! person's real session bus and `~/.config` are never named.
//!
//! The terminal's AppId (`org.quire.Do`, role cli) comes from the cgroup of the pid behind its
//! connection: the binary owns no bus name, so intentd names it by the scope it runs in. The test
//! reads a fake proc root: `quire()` starts the binary through `sh`, which waits for a line on
//! its stdin, so the child's pid (kept by `exec`) is placed in a `vte-spawn-*.scope` before the
//! binary exists, with no timing to rely on.

#[path = "../../intentd/tests/support/apps.rs"]
mod apps;
#[path = "../../intentd/tests/support/bus.rs"]
mod bus;
#[path = "../../intentd/tests/support/memoryd.rs"]
mod memoryd;

use apps::{Answer, FakeSill, SharedMail, serve_mail};
use bus::PrivateBus;
use docket_client::{DbusTransport, Intents};
use docket_core::{ActionRef, AskReason, ConfirmAnswer, ConfirmEnd, ConfirmOffer, GrantScope};
use intentd::{ProcRoot, Running, Setup, start};
use memoryd::FakeMemoryd;
use prov::{Actor, ConfirmId, ConfirmReceipt, InputProof, SpaceScope, UnixSeconds};
use std::path::Path;
use std::process::Output;

fn receipt() -> ConfirmReceipt {
    ConfirmReceipt {
        id: ConfirmId::parse("c-1").expect("id"),
        input: InputProof::HardwareSeat,
        at: UnixSeconds(1),
        covers: prov::Confidentiality::Secret,
    }
}

fn once() -> Answer {
    Answer::With(ConfirmAnswer::Allowed {
        scope: GrantScope::Once,
        receipt: receipt(),
    })
}

fn from_terminal() -> Answer {
    Answer::With(ConfirmAnswer::AllowedFromTerminal { receipt: receipt() })
}

fn refused() -> Answer {
    Answer::With(ConfirmAnswer::Ended(ConfirmEnd::Refused))
}

/// The world: a bus, intentd, a mail app, sill, and a scratch home for `quire-do`.
struct Desk {
    dir: tempfile::TempDir,
    bus: PrivateBus,
    intentd: Option<Running>,
    sill: FakeSill,
    mail: SharedMail,
    /// memoryd, as the audit trail reaches it.
    memoryd: std::sync::Arc<FakeMemoryd>,
    /// The control centre, as sill plays it: a caller that owns sill's names.
    control: Intents<DbusTransport>,
    _connections: Vec<docket_dbus::BusConnection>,
}

impl Desk {
    async fn start() -> Desk {
        let dir = tempfile::tempdir().expect("scratch");
        let data = dir.path().join("data");
        let manifests = data.join("quire/intents");
        std::fs::create_dir_all(&manifests).expect("manifest dir");
        std::fs::write(
            manifests.join("org.quire.Mail.toml"),
            docket_fake::MAIL_MANIFEST,
        )
        .expect("mail manifest");
        // An app that is installed and never running: nothing on the bus owns its name.
        std::fs::write(
            manifests.join("org.quire.Files.toml"),
            docket_fake::FILES_MANIFEST,
        )
        .expect("files manifest");
        let bus = PrivateBus::start(dir.path());

        let mail_connection = bus.connect().await;
        let mail = serve_mail(&mail_connection).await;
        let memory_connection = bus.connect().await;
        let memoryd = FakeMemoryd::recording();
        memoryd.serve(&memory_connection).await;
        let sill_connection = bus.connect().await;
        let sill =
            FakeSill::start(&sill_connection, &["org.quire.Confirm1", "org.quire.Shell"]).await;

        let home = dir.path().display().to_string();
        let env = |key: &str| match key {
            "HOME" => Some(home.clone()),
            "XDG_DATA_HOME" => Some(data.display().to_string()),
            "XDG_DATA_DIRS" => Some(dir.path().join("none").display().to_string()),
            "XDG_CONFIG_HOME" => Some(dir.path().join("config").display().to_string()),
            "XDG_CONFIG_DIRS" => Some(dir.path().join("none").display().to_string()),
            _ => None,
        };
        let mut setup = Setup::from_env(&env).expect("the shipped configuration and the manifests");
        setup.audit_every = std::time::Duration::from_millis(100);
        setup.signals.every = std::time::Duration::from_millis(30);
        setup.proc_root = ProcRoot::Fixture(dir.path().join("proc"));
        let daemon_connection = bus.connect().await;
        let intentd = start(&daemon_connection, None, setup)
            .await
            .expect("intentd starts");

        let control = Intents::over(DbusTransport::new(sill_connection.clone()));
        Desk {
            dir,
            bus,
            intentd: Some(intentd),
            sill,
            mail,
            memoryd,
            control,
            _connections: vec![
                mail_connection,
                memory_connection,
                sill_connection,
                daemon_connection,
            ],
        }
    }

    /// The real `quire-do`, with only what is named here in its environment, running in a
    /// terminal's scope of the fake proc root.
    async fn quire(&self, words: &[&str]) -> Output {
        use tokio::io::AsyncWriteExt;
        let scratch: &Path = self.dir.path();
        let mut child = tokio::process::Command::new("sh")
            .args(["-c", "read _; exec \"$0\" \"$@\" </dev/null"])
            .arg(env!("CARGO_BIN_EXE_quire-do"))
            .args(words)
            .env_clear()
            .env("HOME", scratch)
            .env("XDG_RUNTIME_DIR", scratch)
            .env("XDG_DATA_HOME", scratch.join("data"))
            .env("XDG_DATA_DIRS", scratch.join("none"))
            .env("XDG_CONFIG_HOME", scratch.join("config"))
            .env("DBUS_SESSION_BUS_ADDRESS", self.bus.address())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("quire-do starts");
        let pid = child.id().expect("a pid");
        let at = scratch.join("proc").join(pid.to_string());
        std::fs::create_dir_all(&at).expect("fake proc");
        let slice = "0::/user.slice/user-1000.slice/user@1000.service/app.slice";
        std::fs::write(at.join("cgroup"), format!("{slice}/vte-spawn-1.scope\n")).expect("cgroup");
        let mut go = child.stdin.take().expect("stdin");
        go.write_all(b"\n").await.expect("go");
        drop(go);
        child.wait_with_output().await.expect("quire-do runs")
    }

    async fn stop_intentd(&mut self) {
        if let Some(running) = self.intentd.take() {
            running.stop().await;
        }
    }
}

fn code(out: &Output) -> i32 {
    out.status.code().expect("an exit code")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn say(out: &Output) -> String {
    format!(
        "exit {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        text(&out.stdout),
        text(&out.stderr)
    )
}

fn archive() -> ActionRef {
    ActionRef {
        app: porter_core::AppName::parse("org.quire.Mail").expect("app"),
        name: prov::ActionName::parse("mail.thread.archive").expect("action"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_real_quire_do_reaches_the_real_intentd_on_a_private_bus() {
    let desk = Desk::start().await;
    for key in ["t3", "t4"] {
        desk.mail.0.add_thread(docket_fake::MailThread {
            key: key.into(),
            subject: format!("Thread {key}"),
            from: "someone@example.test".into(),
            body: "words".into(),
        });
    }

    // The installed apps come from the scratch XDG_DATA_DIRS, through intentd's registry.
    let apps = desk.quire(&["apps"]).await;
    assert_eq!(code(&apps), 0, "{}", say(&apps));
    assert!(text(&apps.stdout).contains("mail"), "{}", say(&apps));
    // The two providers intentd hosts itself are listed without any file installed.
    for hosted in ["memory", "companion"] {
        assert!(
            text(&apps.stdout).contains(hosted),
            "{hosted}: {}",
            say(&apps)
        );
    }
    let listed = desk.quire(&["mail", "--list"]).await;
    assert_eq!(code(&listed), 0, "{}", say(&listed));
    assert!(text(&listed.stdout).contains("thread.archive"));

    // A read runs and asks nobody.
    let read = desk.quire(&["mail", "thread.read", "t2"]).await;
    assert_eq!(code(&read), 0, "{}", say(&read));
    assert!(desk.sill.shown().is_empty(), "a read is final: no sheet");

    // A dry run shows the preview and asks nobody either.
    let dry = desk
        .quire(&["mail", "thread.archive", "t1", "--dry-run"])
        .await;
    assert_eq!(code(&dry), 0, "{}", say(&dry));
    assert!(desk.sill.shown().is_empty());
    assert!(!desk.mail.0.is_archived("t1"));

    // A write asks. The person answers once, and it runs.
    desk.sill.answer(vec![once()]);
    let first = desk.quire(&["mail", "thread.archive", "t2"]).await;
    assert_eq!(code(&first), 0, "{}", say(&first));
    assert!(desk.mail.0.is_archived("t2"), "the app did it");
    let shown = desk.sill.shown();
    assert_eq!(shown.len(), 1, "one sheet");
    assert_eq!(
        shown[0].actor,
        Actor::Cli,
        "the sheet says it is the terminal"
    );
    assert_eq!(shown[0].why, [AskReason::FromTerminal]);
    assert_eq!(shown[0].offer, ConfirmOffer::OnceOrFromTerminal);

    // The person says no: exit 4 as well. (Each denial is on a thread of its own: the same
    // action on the same thing, denied once, is refused outright the second time.)
    desk.sill.answer(vec![refused()]);
    let no = desk.quire(&["mail", "thread.archive", "t3"]).await;
    assert_eq!(code(&no), 4, "{}", say(&no));
    assert!(!desk.mail.0.is_archived("t3"));
    assert_eq!(desk.sill.shown().len(), 2);

    // "Allow from the terminal": this once runs, and the next one skips the ask.
    desk.sill.answer(vec![from_terminal()]);
    let granted = desk.quire(&["mail", "thread.archive", "t4"]).await;
    assert_eq!(code(&granted), 0, "{}", say(&granted));
    assert!(desk.mail.0.is_archived("t4"));
    assert_eq!(desk.sill.shown().len(), 3);
    let skipped = desk.quire(&["mail", "thread.archive", "t2"]).await;
    assert_eq!(code(&skipped), 0, "{}", say(&skipped));
    assert_eq!(
        desk.sill.shown().len(),
        3,
        "the standing grant skipped the sheet"
    );

    // The control centre sees the grant and can take it back; then the next write asks again.
    assert_eq!(desk.control.terminal_grants().await, Ok(vec![archive()]));
    desk.control
        .revoke_terminal_grant(archive())
        .await
        .expect("revoked");
    assert_eq!(desk.control.terminal_grants().await, Ok(vec![]));
    let after = desk.quire(&["mail", "thread.archive", "t2"]).await;
    assert_eq!(code(&after), 4, "{}", say(&after));
    assert_eq!(desk.sill.shown().len(), 4, "asked again");

    // The terminal undoes its own last act (the app's undo, through the router).
    assert!(desk.mail.0.is_archived("t2"));
    let undone = desk.quire(&["undo", "--last"]).await;
    assert_eq!(code(&undone), 0, "{}", say(&undone));
    assert!(!desk.mail.0.is_archived("t2"), "undone in the app");

    // A hidden action is refused without asking anyone: exit 3.
    let before = desk.sill.shown().len();
    let hidden = desk.quire(&["memory", "forget", "memory.fact:f1"]).await;
    assert_eq!(code(&hidden), 3, "{}", say(&hidden));
    assert_eq!(
        desk.sill.shown().len(),
        before,
        "nobody is asked about a hidden action"
    );

    // Nothing about the real session was touched.
    assert!(
        !text(&desk.quire(&["apps"]).await.stderr).contains("/run/user"),
        "the private bus only"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn three_refusals_in_a_row_pause_the_terminal_until_the_control_centre_resumes_it() {
    let desk = Desk::start().await;
    for key in ["f1", "f2", "f3"] {
        let hidden = desk
            .quire(&["memory", "forget", &format!("memory.fact:{key}")])
            .await;
        assert_eq!(code(&hidden), 3, "{}", say(&hidden));
    }
    let paused = desk.quire(&["mail", "thread.read", "t2"]).await;
    assert_eq!(code(&paused), 7, "{}", say(&paused));
    // Item 89: the terminal cannot unpause itself, and nothing about asking again changes it.
    let still = desk.quire(&["mail", "thread.read", "t2"]).await;
    assert_eq!(code(&still), 7, "{}", say(&still));
    desk.control
        .resume(SpaceScope::Any)
        .await
        .expect("the control centre resumes");
    let back = desk.quire(&["mail", "thread.read", "t2"]).await;
    assert_eq!(code(&back), 0, "{}", say(&back));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_intentd_stopped_every_call_exits_6_and_sends_nothing() {
    let mut desk = Desk::start().await;
    let up = desk.quire(&["mail", "thread.read", "t2"]).await;
    assert_eq!(code(&up), 0, "{}", say(&up));
    desk.stop_intentd().await;
    for words in [
        vec!["apps"],
        vec!["mail", "--list"],
        vec!["mail", "thread.read", "t2"],
        vec!["mail", "thread.archive", "t2"],
        vec!["undo", "--last"],
    ] {
        let out = desk.quire(&words).await;
        assert_eq!(code(&out), 6, "{words:?}: {}", say(&out));
        assert!(
            text(&out.stderr).contains("intentd is not running"),
            "{}",
            say(&out)
        );
        assert_eq!(text(&out.stdout), "");
    }
    assert!(desk.sill.shown().is_empty(), "no sheet was raised");
    assert!(!desk.mail.0.is_archived("t2"));
}

/// What the terminal did reaches memoryd as the audit trail: one `docket.call` record, in the
/// Space the call ran in, acted by the terminal, naming the thing it read.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn what_the_terminal_did_is_in_memory_as_audit_records() {
    let desk = Desk::start().await;
    let read = desk.quire(&["mail", "thread.read", "t2"]).await;
    assert_eq!(code(&read), 0, "{}", say(&read));
    let mut found = None;
    for _ in 0..100 {
        found = desk
            .memoryd
            .stored()
            .into_iter()
            .find(|r| r.body.kind().as_str() == "docket.call");
        if found.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let record = found.expect("the call reached memoryd");
    assert_eq!(record.actor, Actor::Cli);
    assert_eq!(record.effect, prov::Effect::Read);
    assert_eq!(
        record.space,
        prov::SpaceId::desktop(),
        "a terminal's session is in the desktop Space"
    );
    let almanac_core::EventBody::Area(payload) = &record.body else {
        panic!("{:?}", record.body)
    };
    assert!(payload.json.as_str().contains("mail.thread.read"));
    assert!(
        payload
            .things
            .iter()
            .any(|(t, _)| t.thing.key.as_str() == "t2"),
        "it names what it touched: {:?}",
        payload.things
    );
}

async fn first<T>(stream: &mut (impl futures_util::Stream<Item = T> + Unpin)) -> T {
    use futures_util::StreamExt;
    tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
        .await
        .expect("the signal comes")
        .expect("a signal")
}

/// The breaker pausing the terminal's session is said on the control interface, content-free.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_breaker_pausing_the_terminal_is_a_signal() {
    let desk = Desk::start().await;
    let listener = desk.bus.connect().await;
    let control = docket_dbus::ControlProxy::new(&listener)
        .await
        .expect("proxy");
    let mut tripped = control.receive_breaker_tripped().await.expect("subscribed");
    for key in ["f1", "f2", "f3"] {
        let hidden = desk
            .quire(&["memory", "forget", &format!("memory.fact:{key}")])
            .await;
        assert_eq!(code(&hidden), 3, "{}", say(&hidden));
    }
    let signal = first(&mut tripped).await;
    let session = signal.args().expect("args").session().to_string();
    assert!(
        session.starts_with("s-"),
        "a session id and nothing else: {session}"
    );
}

/// A change the terminal makes that can be undone is a row in the journal, and the control
/// centre is told how many rows there are.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_undoable_change_is_a_journal_changed_signal_with_the_row_count() {
    let desk = Desk::start().await;
    let listener = desk.bus.connect().await;
    let control = docket_dbus::ControlProxy::new(&listener)
        .await
        .expect("proxy");
    let mut changed = control.receive_journal_changed().await.expect("subscribed");
    desk.sill.answer(vec![once()]);
    let archived = desk.quire(&["mail", "thread.archive", "t2"]).await;
    assert_eq!(code(&archived), 0, "{}", say(&archived));
    let signal = first(&mut changed).await;
    assert_eq!(*signal.args().expect("args").rows(), 1);
}

/// An installed app that is not running (and cannot be started) is exit 6, "unavailable", not
/// exit 5, "the app failed" (interface-asks 105, cli.md section 4).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_app_that_is_absent_is_exit_6() {
    let desk = Desk::start().await;
    let out = desk.quire(&["files", "file.read", "f1"]).await;
    assert_eq!(code(&out), 6, "{}", say(&out));
    // Stdout is not a terminal here, so the error is the JSON one.
    assert!(
        text(&out.stdout).contains("\"kind\":\"app_unavailable\""),
        "{}",
        say(&out)
    );
}

/// The role is the cli role by the executable, never by anything `quire-do` says: another
/// process that owns no name and is not `quire-do` is nobody, and `quire-do` run as the person's
/// control centre would still be a terminal.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_process_that_is_not_quire_do_is_not_the_terminal() {
    let desk = Desk::start().await;
    // This test binary owns no name and has no cgroup in the fake proc root: intentd names it nobody.
    let stranger = DbusTransport::new(desk.bus.connect().await);
    let intents = Intents::over(stranger);
    let reply = intents.manifests().await;
    assert!(
        matches!(
            reply,
            Err(docket_client::ClientError::Refused(
                docket_core::WireRefusal::NotAllowed
            ))
        ),
        "{reply:?}"
    );
}
