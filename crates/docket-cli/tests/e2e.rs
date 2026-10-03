//! `quire-do` end to end: the real binary, intentd's real `start` (the router over `DbusLink`,
//! `SheetConfirmer` and `FileGrants`, serving `org.quire.Intents1`), a mail app that is
//! docket-fake's `FakeMail` behind `docket_client::serve_on`, and sill's `Confirm1` as a fake
//! that answers from a script. All of it on a private `dbus-daemon`, with a scratch HOME and
//! XDG directories (the app's manifest is installed under the scratch `XDG_DATA_DIRS`): the
//! person's real session bus and `~/.config` are never named.
//!
//! The terminal's AppId (`org.quire.Do`, role cli) comes from the pid behind its connection:
//! the binary owns no bus name, so intentd names it by its executable.

#[path = "../../intentd/tests/support/apps.rs"]
mod apps;
#[path = "../../intentd/tests/support/bus.rs"]
mod bus;

use apps::{Answer, FakeSill, SharedMail, serve_mail};
use bus::PrivateBus;
use docket_client::{DbusTransport, Intents};
use docket_core::{ActionRef, AskReason, ConfirmAnswer, ConfirmEnd, ConfirmOffer, GrantScope};
use intentd::{Running, Setup, start};
use prov::{Actor, ConfirmId, ConfirmReceipt, InputProof, SpaceScope, UnixSeconds};
use std::path::Path;
use std::process::Output;

const MEMORY_MANIFEST: &str = include_str!("../../../manifests/org.quire.Memory.toml");

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
        std::fs::write(manifests.join("org.quire.Memory.toml"), MEMORY_MANIFEST)
            .expect("memory manifest");
        let bus = PrivateBus::start(dir.path());

        let mail_connection = bus.connect().await;
        let mail = serve_mail(&mail_connection).await;
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
        let setup = Setup::from_env(&env).expect("the shipped configuration and the manifests");
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
            control,
            _connections: vec![mail_connection, sill_connection, daemon_connection],
        }
    }

    /// The real `quire-do`, with only what is named here in its environment.
    async fn quire(&self, words: &[&str]) -> Output {
        let scratch: &Path = self.dir.path();
        tokio::process::Command::new(env!("CARGO_BIN_EXE_quire-do"))
            .args(words)
            .env_clear()
            .env("HOME", scratch)
            .env("XDG_RUNTIME_DIR", scratch)
            .env("XDG_DATA_HOME", scratch.join("data"))
            .env("XDG_DATA_DIRS", scratch.join("none"))
            .env("XDG_CONFIG_HOME", scratch.join("config"))
            .env("DBUS_SESSION_BUS_ADDRESS", self.bus.address())
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .expect("quire-do runs")
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

/// The role is the cli role by the executable, never by anything `quire-do` says: another
/// process that owns no name and is not `quire-do` is nobody, and `quire-do` run as the person's
/// control centre would still be a terminal.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_process_that_is_not_quire_do_is_not_the_terminal() {
    let desk = Desk::start().await;
    // This test binary owns no name and runs no known executable: intentd names it nobody.
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
