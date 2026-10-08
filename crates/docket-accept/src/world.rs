//! The acceptance world: a private bus, the five real daemons as processes (inferd and memoryd as
//! the packaged binaries of their repos, intentd, readerd and companiond), and the fakes on the
//! bus around them: sill's `Confirm1` and a mail provider. The model is inferd's replay engine
//! playing a cassette. Everything is scratch: HOME, every XDG directory and the bus.
//!
//! Teardown is certain: every daemon is a `Reaped` child (killed by PID and waited for on drop,
//! with a watchdog if the test process itself dies) and the bus is `docket-testbus`'s. Waits are
//! on bus events (a name appearing); the long bounds exist only to turn a hang into a failure.

use crate::confirm::{self, Sheet};
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
    /// almanac's packaged `memoryd`, built with `test-keys`.
    pub memoryd: PathBuf,
    /// porter's packaged `inferd`.
    pub inferd: PathBuf,
    /// `accept-quire-do`.
    pub quire_do: PathBuf,
}

/// The model's script: the text of a cassette file (`dev/accept/cassettes`), written into the
/// scratch config for inferd's replay engine to play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cassette(pub &'static str);

/// Where the model's answers come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelSource {
    /// inferd's replay engine plays this cassette (the text of a `.jsonl` file).
    Scripted(String),
    /// A real engine: this is the body of an `inferd.toml` (engines, `[ai]` rows) the owner wrote.
    /// The world appends the `[callers.apps]` table, so the body must not hold one.
    Live(String),
}

/// The person's `agent.acp.expose`, written into the scratch settings file. intentd honours the
/// roles of `org.quire.Acp` only while it is on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AcpSetting {
    /// No settings file: the feature is off.
    #[default]
    Off,
    /// `[agent.acp] expose = "on"`.
    On,
}

/// What a world does besides what its model source says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// Where the scratch root is made, and kept when the run ends (a live run's traces live in
    /// it). Absent: a temporary directory that goes with the world.
    pub keep_in: Option<PathBuf>,
    /// Turn on the model tap in every daemon (`DOCKET_MODEL_TRACE`): every request a daemon
    /// sends a model and its answer, appended to `<scratch>/model.jsonl`. THIS WRITES PROMPTS TO
    /// DISK, in the scratch root.
    pub tap: TapMode,
    /// The accountd binary to run on the private bus (a cloud run's key store): a build with the
    /// `test-proc-root` and `test-keys` features, as `ACCEPT_ACCOUNTD` names it.
    pub accountd: Option<PathBuf>,
    /// A catalogue directory whose `*.toml` entries are copied into the scratch root before
    /// inferd starts (see `live::catalog`).
    pub catalog: Option<PathBuf>,
    /// Whether the person switched the ACP edge on.
    pub acp: AcpSetting,
}

/// Copies the run's catalogue into the scratch root; a missing source is a failed start.
pub(crate) fn copy_catalog(from: Option<&Path>, root: &Path) {
    if let Some(from) = from {
        crate::live::catalog::copy_entries(from, &crate::live::catalog::world_dir(root))
            .unwrap_or_else(|e| panic!("catalogue {}: {e}", from.display()));
    }
}

/// Whether the daemons tap their model link.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TapMode {
    /// No daemon keeps a copy of any prompt.
    #[default]
    Off,
    /// Every daemon appends its exchanges to the scratch root's `model.jsonl`.
    On,
}

/// A scratch root: temporary, or kept where the run was told to leave it.
#[derive(Debug)]
pub struct Scratch {
    path: PathBuf,
    _owned: Option<tempfile::TempDir>,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        crate::runlink::unlink_run(&self.path);
    }
}

impl Scratch {
    pub(crate) fn made(keep_in: Option<&Path>) -> std::io::Result<Self> {
        match keep_in {
            Some(root) => {
                std::fs::create_dir_all(root)?;
                let dir = tempfile::Builder::new().prefix("world-").tempdir_in(root)?;
                Ok(Self {
                    path: dir.keep(),
                    _owned: None,
                })
            }
            None => {
                let dir = tempfile::tempdir()?;
                Ok(Self {
                    path: dir.path().to_owned(),
                    _owned: Some(dir),
                })
            }
        }
    }

    /// The root.
    pub fn path(&self) -> &Path {
        &self.path
    }
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
    /// What the mail provider was asked and sent.
    pub mail: MailLog,
    /// A connection that owns `org.quire.Shell` and `org.quire.Confirm1`: sill.
    pub sill: zbus::Connection,
    _helpers: Vec<zbus::Connection>,
    /// The scratch root.
    pub dir: Scratch,
    bus: PrivateBus,
}

impl std::fmt::Debug for World {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("World")
    }
}

pub(crate) fn env_of(dir: &Path) -> Vec<(&'static str, String)> {
    let at = |p: &str| dir.join(p).display().to_string();
    vec![
        ("HOME", dir.display().to_string()),
        // What systemd gives a user unit: an engine's compilers (vLLM's Triton runs gcc, which
        // finds `ld` on PATH) need it.
        ("PATH", "/usr/bin:/bin".to_owned()),
        ("XDG_DATA_HOME", at("data")),
        ("XDG_DATA_DIRS", at("none")),
        ("XDG_CONFIG_HOME", at("config")),
        ("XDG_CONFIG_DIRS", at("none")),
        ("XDG_CACHE_HOME", at("cache")),
        (
            "XDG_RUNTIME_DIR",
            crate::runlink::short_run(dir).display().to_string(),
        ),
    ]
}

/// The engines' compile caches, when the run names a directory for them (`DOCKET_LIVE_ENGINE_CACHE`,
/// set by the live scripts): a world is fresh each time, so without it vLLM compiles for minutes
/// in every world. inferd's engines inherit its environment.
fn engine_cache() -> Vec<(&'static str, String)> {
    std::env::var("DOCKET_LIVE_ENGINE_CACHE")
        .map(|root| {
            vec![
                ("VLLM_CACHE_ROOT", format!("{root}/vllm")),
                ("TRITON_CACHE_DIR", format!("{root}/triton")),
                ("TORCHINDUCTOR_CACHE_DIR", format!("{root}/inductor")),
            ]
        })
        .unwrap_or_default()
}

/// What only one daemon is told. memoryd's packaged binary runs a test build: its keys are a file
/// (there is no Secret Service on the private bus), its sandbox stays ON, and both daemons read
/// their callers from the scratch fake proc root instead of `/proc`.
fn extra_env(dir: &Path, name: &str) -> Vec<(&'static str, String)> {
    let proc_root = dir.join("proc").display().to_string();
    match name {
        "memoryd" => vec![
            (
                "MEMORYD_KEYS",
                format!("file:{}", dir.join("keys/memoryd.keys").display()),
            ),
            ("MEMORYD_PROC_ROOT", proc_root),
        ],
        "inferd" => [("INFERD_PROC_ROOT", proc_root)]
            .into_iter()
            .chain(engine_cache())
            .collect(),
        // A cloud run's key store: its caller table is the scratch one, and its keys a file in
        // the scratch root (a build with the `test-keys` feature; see docs/live-eval.md).
        "accountd" => vec![
            (
                "ACCOUNTD_KEYS",
                format!("file:{}", dir.join("keys/accountd.keys").display()),
            ),
            ("ACCOUNTD_PROC_ROOT", proc_root),
        ],
        // intentd and companiond tell a terminal from its cgroup, read from the scratch root.
        "intentd" => vec![("INTENTD_PROC_ROOT", proc_root)],
        "companiond" => vec![("COMPANIOND_PROC_ROOT", proc_root)],
        _ => Vec::new(),
    }
}

/// What a process of the fake proc root is: the unit it runs in, or the app scope it is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cgroup<'a> {
    /// A user service unit, `<name>.service`.
    Unit(&'a str),
    /// An app's scope, `app-<app>-1.scope`.
    AppScope(&'a str),
    /// A named transient user scope outside the `app-` namespace, `<name>.scope`.
    Scope(&'a str),
}

impl Cgroup<'_> {
    /// The one line of `/proc/<pid>/cgroup`.
    fn line(self) -> String {
        let slice = "0::/user.slice/user-1000.slice/user@1000.service/app.slice";
        match self {
            Cgroup::Unit(name) => format!("{slice}/{name}.service\n"),
            Cgroup::AppScope(app) => format!("{slice}/app-{app}-1.scope\n"),
            Cgroup::Scope(name) => format!("{slice}/{name}.scope\n"),
        }
    }
}

/// Puts process `pid` in the fake proc root `<root>/proc`, in `cgroup`.
pub fn place(root: &Path, pid: u32, cgroup: Cgroup<'_>) {
    write(&root.join(format!("proc/{pid}/cgroup")), &cgroup.line());
}

const CALLERS: &str = "[callers.apps]\n\"org.quire.Memory\" = [\"memoryd.service\"]\n\"org.quire.Intents\" = [\"intentd.service\"]\n\"org.quire.Companion\" = [\"companiond.service\"]\n\"org.quire.Reader\" = [\"readerd.service\"]\n";

/// inferd's configuration. A cassette: the replay engine `scripted` playing the scratch cassette
/// (recording its requests when `record` names a file). A live source: the owner's body as it is.
/// Either way, then the callers by unit, as `inferd.toml` names them.
pub fn inferd_toml(root: &Path, model: &ModelSource, record: Option<&Path>) -> String {
    let engines = match model {
        ModelSource::Scripted(_) => {
            let record = record
                .map(|p| format!("record = {:?}\n", p.display().to_string()))
                .unwrap_or_default();
            format!(
                "[engines.scripted]\nreplay = {:?}\n{record}\n",
                root.join("cassette.jsonl").display().to_string()
            )
        }
        ModelSource::Live(body) => format!("{body}\n"),
    };
    format!("{engines}{CALLERS}")
}

/// accountd's caller table for a cloud run: inferd is the one porter daemon, and may resolve keys.
pub(crate) const ACCOUNTD_CALLERS: &str = r#"[[caller]]
app = "org.quire.Inference"
unit = "inferd.service"
role = "porter_daemon"
"#;

/// memoryd's callers file: the units that may call it, with the roles the router and the shell
/// are told apart by. The shell has both rows (sill.service, or the scope sill-session starts);
/// the run places the test process in the scope.
const MEMORY_CALLERS: &str = r#"[[caller]]
app = "org.quire.Intents"
unit = "intentd.service"
role = "agent"

[[caller]]
app = "org.quire.Shell"
unit = "sill.service"
role = "sheet_host"

[[caller]]
app = "org.quire.Shell"
unit = "sill-shell.scope"
role = "sheet_host"

[[caller]]
app = "org.quire.Companion"
unit = "companiond.service"
role = "agent"

[[caller]]
app = "org.quire.Reader"
unit = "readerd.service"
role = "agent"
"#;

pub(crate) fn spawn(
    dir: &Path,
    bus: &str,
    name: &'static str,
    binary: &Path,
    tap: Option<&Path>,
) -> std::io::Result<(&'static str, Reaped)> {
    let log = std::fs::File::create(dir.join("logs").join(format!("{name}.log")))?;
    let mut command = Command::new(binary);
    command
        .env_clear()
        .envs(env_of(dir))
        .envs(extra_env(dir, name))
        .env("DBUS_SESSION_BUS_ADDRESS", bus)
        .env("DBUS_SYSTEM_BUS_ADDRESS", bus)
        .envs(tap.map(|file| (docket_dbus::tap::TRACE_VAR, file.to_owned())))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log));
    Ok((name, Reaped::spawn(&mut command)?))
}

impl Drop for World {
    fn drop(&mut self) {
        let text = self.recorded_requests();
        if !text.is_empty() {
            eprintln!("---- planner and reader requests (ACCEPT_RECORD)\n{text}");
        }
    }
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

pub(crate) fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("dirs");
    std::fs::write(path, text).expect("file");
}

/// `spaces.toml`: the Space `work`, and `desktop` where intentd places the records that name no
/// Space (a confirmation, an undo), both sealed with fixed replicas.
const SPACES: &str = r#"[[spaces]]
id = "work"
created = 0
replica = "00000000000000000000000000000001"
vault = "sealed"
format = 1

[[spaces]]
id = "desktop"
created = 0
replica = "00000000000000000000000000000002"
vault = "sealed"
format = 1
"#;

impl World {
    /// Starts everything, in dependency order: the bus, sill, inferd (replaying `cassette`),
    /// memoryd, intentd (with the mail manifest installed), the mail provider, readerd, companiond.
    pub async fn start(binaries: &Binaries, consent: Consent, cassette: Cassette) -> World {
        let model = ModelSource::Scripted(cassette.0.to_owned());
        World::start_model(binaries, consent, &model, &Options::default()).await
    }

    /// [`World::start`] with the person's `agent.acp.expose` switched on, for a run with an editor.
    pub async fn start_acp(binaries: &Binaries, consent: Consent, cassette: Cassette) -> World {
        let model = ModelSource::Scripted(cassette.0.to_owned());
        let options = Options {
            acp: AcpSetting::On,
            ..Options::default()
        };
        World::start_model(binaries, consent, &model, &options).await
    }

    /// [`World::start`] with any model source and options: a cassette or a real engine, the
    /// scratch root kept or not, the daemons' model tap on or off.
    pub async fn start_model(
        binaries: &Binaries,
        consent: Consent,
        model: &ModelSource,
        options: &Options,
    ) -> World {
        let dir = Scratch::made(options.keep_in.as_deref()).expect("scratch");
        let root = dir.path();
        for sub in ["logs", "data", "config", "cache", "run"] {
            std::fs::create_dir_all(root.join(sub)).expect("dirs");
        }
        std::fs::set_permissions(
            root.join("run"),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .expect("run dir mode");
        crate::runlink::link_run(root).expect("short runtime name");
        let bus = PrivateBus::start(root);
        let address = bus.address().to_owned();

        // sill is on the bus before any daemon needs it.
        let sill = bus.connect().await;
        let (sheet, confirms) = confirm::serve(&sill).await.expect("Confirm1");

        // Files the daemons read: memoryd's callers, its Spaces, intentd's manifest and consent.
        // The test process is the shell (sill): the fake proc root says so.
        let record = std::env::var_os("ACCEPT_RECORD").map(|_| root.join("record.jsonl"));
        place(root, std::process::id(), Cgroup::Scope("sill-shell"));
        write(
            &root.join("config/quire/memory-callers.toml"),
            MEMORY_CALLERS,
        );
        write(&root.join("data/quire/memory/spaces.toml"), SPACES);
        if let ModelSource::Scripted(cassette) = model {
            write(&root.join("cassette.jsonl"), cassette);
        }
        write(
            &root.join("config/quire/inferd.toml"),
            &inferd_toml(root, model, record.as_deref()),
        );
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

        if options.acp == AcpSetting::On {
            write(
                &root.join("config/docket/settings.toml"),
                "[agent.acp]\nexpose = \"on\"\n",
            );
        }
        copy_catalog(options.catalog.as_deref(), root);
        let tap = match options.tap {
            TapMode::On => Some(root.join("model.jsonl")),
            TapMode::Off => None,
        };
        let mut daemons = Vec::new();
        let mut start = async |name: &'static str, binary: &Path, owns: &str| {
            let daemon =
                spawn(root, &address, name, binary, tap.as_deref()).expect("daemon starts");
            place(root, daemon.1.pid(), Cgroup::Unit(name));
            daemons.push(daemon);
            until_owned(&sill, owns).await;
        };
        if let Some(accountd) = &options.accountd {
            write(&root.join("config/porter/callers.toml"), ACCOUNTD_CALLERS);
            start("accountd", accountd, "org.quire.Accounts1").await;
        }
        start("inferd", &binaries.inferd, "org.quire.Inference1").await;
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
            mail: log,
            sill,
            _helpers: vec![provider_connection],
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

    /// inferd's audit trail so far: one entry per finished model turn, with the app that asked
    /// (never the content).
    pub fn model_turns(&self) -> Vec<porter_infer::AuditEntry> {
        std::fs::read_to_string(
            self.dir
                .path()
                .join(".local/state/quire/inferd/audit.jsonl"),
        )
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
    }

    /// The requests inferd's replay engine was asked, one JSON line each, when the run was started
    /// with `ACCEPT_RECORD=1`; empty otherwise.
    pub fn recorded_requests(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("record.jsonl")).unwrap_or_default()
    }

    /// The real `quire-do` (`binary`: `accept-quire-do`) with `words`, run as a terminal's child:
    /// it starts through `sh`, which waits for a line on stdin, so its pid (kept by `exec`) is
    /// placed in a `vte-spawn-*` scope of the scratch proc root before the program exists. Its
    /// environment is the scratch one and the private bus, nothing else.
    pub async fn quire_do(&self, binary: &Path, words: &[&str]) -> std::process::Output {
        use std::io::Write;
        let mut child = Command::new("sh")
            .args(["-c", "read _; exec \"$0\" \"$@\" </dev/null"])
            .arg(binary)
            .args(words)
            .env_clear()
            .envs(env_of(self.dir.path()))
            .env("DBUS_SESSION_BUS_ADDRESS", self.address())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("quire-do starts");
        place(self.dir.path(), child.id(), Cgroup::Scope("vte-spawn-1"));
        let mut go = child.stdin.take().expect("stdin");
        go.write_all(b"\n").expect("go");
        drop(go);
        tokio::task::spawn_blocking(move || child.wait_with_output().expect("quire-do runs"))
            .await
            .expect("joined")
    }

    /// Every daemon's standard error, for a failing test to print.
    pub fn logs(&self) -> String {
        self.daemons
            .iter()
            .map(|(name, _)| {
                let text =
                    std::fs::read_to_string(self.dir.path().join(format!("logs/{name}.log")))
                        .unwrap_or_default();
                format!("---- {name}\n{text}")
            })
            .collect()
    }
}
