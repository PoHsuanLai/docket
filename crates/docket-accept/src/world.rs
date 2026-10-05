//! The acceptance world: a private bus, the four real daemons as processes (memoryd, intentd,
//! readerd, companiond), and the fakes on the bus around them: sill's `Confirm1`, the scripted
//! inferd, and a mail provider. Everything is scratch: HOME, every XDG directory and the bus.
//!
//! Teardown is certain: every daemon is a `Reaped` child (killed by PID and waited for on drop,
//! with a watchdog if the test process itself dies) and the bus is `docket-testbus`'s. Waits are
//! on bus events (a name appearing); the long bounds exist only to turn a hang into a failure.

use crate::confirm::{self, Sheet};
use crate::inferd::{self, Model};
use crate::provider::{AcceptMail, MailLog, QuietWindow};
use docket_core::{ConfirmRequest, ValidManifest};
use docket_router::parse;
use docket_testbus::{PrivateBus, Reaped};
use futures_util::StreamExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use tokio::sync::mpsc;
use zbus::fdo::DBusProxy;

/// How long a wait for a bus event may take before the test fails instead of hanging.
pub const GIVE_UP: Duration = Duration::from_secs(120);

/// The manifest the mail provider answers for, installed for intentd to read.
pub const MAIL_MANIFEST: &str = include_str!("../../../dev/accept/fixtures/org.quire.Mail.toml");

/// Where the daemon binaries are (`env!("CARGO_BIN_EXE_*")` of the test).
#[derive(Debug, Clone)]
pub struct Binaries {
    /// `accept-intentd`.
    pub intentd: PathBuf,
    /// `accept-companiond`.
    pub companiond: PathBuf,
    /// `accept-readerd`.
    pub readerd: PathBuf,
    /// `accept-memoryd`.
    pub memoryd: PathBuf,
}

/// What a world starts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consent {
    /// The companion's standing consent for Mail's classes in the Space is already on file, so
    /// the only sheet of a run is the one the call itself asks.
    Standing,
    /// No grants on file: the first use of each class asks too.
    FirstUse,
}

/// The running world.
pub struct World {
    // Declared first: the daemons are killed before anything they talk to goes.
    daemons: Vec<(&'static str, Reaped)>,
    /// What sill's `Confirm1` was shown, and what the person will do with it.
    pub sheet: Sheet,
    /// The same, as a stream: one item per sheet shown.
    pub confirms: mpsc::UnboundedReceiver<ConfirmRequest>,
    /// The scripted model and what it was asked.
    pub model: Model,
    /// What the mail provider was asked and sent.
    pub mail: MailLog,
    /// A connection that owns `org.quire.Shell` and `org.quire.Confirm1`: sill.
    pub sill: zbus::Connection,
    _helpers: Vec<zbus::Connection>,
    /// The scratch root.
    pub dir: tempfile::TempDir,
    bus: PrivateBus,
}

impl std::fmt::Debug for World {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("World")
    }
}

fn env_of(dir: &Path) -> Vec<(&'static str, String)> {
    let at = |p: &str| dir.join(p).display().to_string();
    vec![
        ("HOME", dir.display().to_string()),
        ("XDG_DATA_HOME", at("data")),
        ("XDG_DATA_DIRS", at("none")),
        ("XDG_CONFIG_HOME", at("config")),
        ("XDG_CONFIG_DIRS", at("none")),
        ("XDG_CACHE_HOME", at("cache")),
        ("XDG_RUNTIME_DIR", at("run")),
    ]
}

fn spawn(
    dir: &Path,
    bus: &str,
    name: &'static str,
    binary: &Path,
) -> std::io::Result<(&'static str, Reaped)> {
    let log = std::fs::File::create(dir.join("logs").join(format!("{name}.log")))?;
    let mut command = Command::new(binary);
    command
        .env_clear()
        .envs(env_of(dir))
        .env("DBUS_SESSION_BUS_ADDRESS", bus)
        .env("DBUS_SYSTEM_BUS_ADDRESS", bus)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log));
    Ok((name, Reaped::spawn(&mut command)?))
}

/// Waits until `name` has an owner on the bus. Subscribes first, so an owner that appears
/// between the check and the wait is not missed.
pub async fn until_owned(connection: &zbus::Connection, name: &str) {
    let dbus = DBusProxy::new(connection).await.expect("dbus");
    let mut changes = dbus
        .receive_name_owner_changed_with_args(&[(0, name)])
        .await
        .expect("subscribed");
    let bus_name = zbus::names::BusName::try_from(name).expect("name");
    let wait = async {
        while !dbus.name_has_owner(bus_name.clone()).await.unwrap_or(false) {
            changes.next().await;
        }
    };
    tokio::time::timeout(GIVE_UP, wait)
        .await
        .unwrap_or_else(|_| panic!("{name} never appeared on the bus"));
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("dirs");
    std::fs::write(path, text).expect("file");
}

/// `spaces.toml`: the Space `work` exists, sealed, with a fixed replica.
const SPACES: &str = r#"[[spaces]]
id = "work"
created = 0
replica = "00000000000000000000000000000001"
vault = "sealed"
format = 1
"#;

/// The model's answer to intentd's policy writer for the acceptance turns: Mail's actions up to
/// Outbound, at most 12 things, no recipient named (the person named none).
pub const WRITER_DRAFT: &str = r#"{"actions":["org.quire.Mail mail.thread.find","org.quire.Mail mail.thread.read","org.quire.Mail mail.contact.search","org.quire.Mail mail.message.forward","org.quire.Mail mail.message.send"],"apps":[{"app":"org.quire.Mail","up_to":"outbound"}],"kinds":["mail.thread","mail.contact"],"ceiling":"outbound","max_count":12,"recipients":[],"destinations":[],"paths":[]}"#;

impl World {
    /// Starts everything, in dependency order: the bus, sill and the scripted model, memoryd,
    /// intentd (with the mail manifest installed), the mail provider, readerd, companiond.
    pub async fn start(binaries: &Binaries, consent: Consent) -> World {
        let dir = tempfile::tempdir().expect("scratch");
        let root = dir.path();
        for sub in ["logs", "data", "config", "cache", "run"] {
            std::fs::create_dir_all(root.join(sub)).expect("dirs");
        }
        std::fs::set_permissions(
            root.join("run"),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .expect("run dir mode");
        let bus = PrivateBus::start(root);
        let address = bus.address().to_owned();

        // sill and the model are on the bus before any daemon needs them.
        let sill = bus.connect().await;
        let (sheet, confirms) = confirm::serve(&sill).await.expect("Confirm1");
        let model_connection = bus.connect().await;
        let model = inferd::serve(&model_connection).await.expect("Inference1");
        model.writer(WRITER_DRAFT);

        // Files the daemons read: memoryd's callers (the router is intentd's executable, the
        // shell is this test process), its Spaces, intentd's manifest and consent.
        let canon = |p: &Path| p.canonicalize().expect("binary path").display().to_string();
        let me = std::env::current_exe().expect("test executable");
        write(
            &root.join("config/quire/memory-callers.toml"),
            &format!(
                "router = [\"{}\"]\nshell = [\"{}\"]\n",
                canon(&binaries.intentd),
                canon(&me)
            ),
        );
        write(&root.join("data/quire/memory/spaces.toml"), SPACES);
        write(
            &root.join("data/quire/intents/org.quire.Mail.toml"),
            MAIL_MANIFEST,
        );
        if consent == Consent::Standing {
            write(
                &root.join("data/quire/intents/grants.json"),
                &crate::grants::standing_json("work"),
            );
        }

        let mut daemons = Vec::new();
        let mut start = async |name: &'static str, binary: &Path, owns: &str| {
            daemons.push(spawn(root, &address, name, binary).expect("daemon starts"));
            until_owned(&sill, owns).await;
        };
        start("memoryd", &binaries.memoryd, "org.quire.Memory1").await;
        start("intentd", &binaries.intentd, "org.quire.Intents1").await;

        // The mail provider: its own connection, its own name, the manifest's wire.
        let provider_connection = bus.connect().await;
        let manifest: ValidManifest = parse(MAIL_MANIFEST).expect("the mail manifest");
        let space = prov::SpaceId::parse("work").expect("space");
        let (mail, log) = AcceptMail::new(manifest, space);
        let quiet = QuietWindow(porter_core::AppName::parse("org.quire.Mail").expect("app"));
        docket_client::serve_on(&provider_connection, mail, quiet.clone(), quiet)
            .await
            .expect("the mail provider serves");

        start("readerd", &binaries.readerd, "org.quire.Reader1").await;
        start("companiond", &binaries.companiond, "org.quire.Companion1").await;
        World {
            daemons,
            sheet,
            confirms,
            model,
            mail: log,
            sill,
            _helpers: vec![model_connection, provider_connection],
            dir,
            bus,
        }
    }

    /// A fresh connection to the private bus.
    pub async fn connect(&self) -> zbus::Connection {
        self.bus.connect().await
    }

    /// The bus address.
    pub fn address(&self) -> &str {
        self.bus.address()
    }

    /// Every daemon's standard error, for a failing test to print.
    pub fn logs(&self) -> String {
        self.daemons
            .iter()
            .map(|(name, _)| {
                let text = std::fs::read_to_string(self.dir.path().join(format!("logs/{name}.log")))
                    .unwrap_or_default();
                format!("---- {name}\n{text}")
            })
            .collect()
    }
}
