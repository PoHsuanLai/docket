//! The acceptance world: a private bus, the real daemons as processes (accountd, inferd and memoryd as
//! the packaged binaries of their repos, intentd, readerd and companiond), and the fakes on the
//! bus around them: sill's `Confirm1` and a mail provider. The model is inferd's replay engine
//! playing a cassette. Everything is scratch: HOME, every XDG directory and the bus.
//!
//! Teardown is certain: every daemon is a `Reaped` child (killed by PID and waited for on drop,
//! with a watchdog if the test process itself dies) and the bus is `docket-testbus`'s. Waits are
//! on bus events (a name appearing); the long bounds exist only to turn a hang into a failure.

//!
//! The parts: `env` (what a daemon's process is given), `proc` (the fake `/proc`), `files` (what
//! the daemons read from the scratch root), `start` (starting a world) and `ops` (what a running
//! world does).

mod env;
mod files;
mod ops;
mod proc;
mod start;

pub use env::{DaemonEnv, env_of, extra_env, spawn};
pub use files::{
    ACCOUNTD_CALLERS, INFERD_CALLERS, MEMORY_CALLERS, SPACES, inferd_toml, write,
    write_system_callers,
};
pub use proc::{Cgroup, as_app, link_image, place, restore_unit, unit_main};
pub use start::until_owned;

use crate::confirm::Sheet;
use crate::provider::{AppLog, Focus};
use crate::things::App;
use docket_core::ConfirmRequest;
use docket_testbus::{PrivateBus, Reaped};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc;

/// How long a wait for a bus event may take before the test fails instead of hanging. Shorter than
/// the 2 minutes nextest allows a test (`.config/nextest.toml`), so the failure names what was
/// waited for instead of being a bare kill.
pub const GIVE_UP: Duration = Duration::from_secs(60);

/// Runs `wait`, failing the test, naming `what` it waited for, if it takes longer than [`GIVE_UP`].
/// Every call over the bus and every wait on a peer goes through it: a zbus method call has no
/// timeout of its own, and a peer that never answers would hold the test for ever.
pub async fn within<T>(what: &str, wait: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(GIVE_UP, wait)
        .await
        .unwrap_or_else(|_| panic!("timed out after {GIVE_UP:?} waiting for {what}"))
}

/// The manifest the mail provider answers for, installed for intentd to read.
pub const MAIL_MANIFEST: &str = include_str!("../../../../dev/accept/fixtures/org.quire.Mail.toml");

/// Where the daemon binaries are (`env!("CARGO_BIN_EXE_*")` of the test), and the directory the
/// engines of inferd keep their compile caches in, which only a main or a test names.
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
    /// porter's packaged `accountd`, built with `test-keys`: it holds the AI keys.
    pub accountd: PathBuf,
    /// porter's packaged `inferd`.
    pub inferd: PathBuf,
    /// `accept-quire-do`.
    pub quire_do: PathBuf,
    /// `accept-actions-mcp`: the bridge an external agent starts.
    pub actions_mcp: PathBuf,
    /// `accept-bus-relay`: a bus connection that is a process of its own ([`World::connect_as`]).
    pub relay: PathBuf,
    /// A directory shared by every world's inferd for its engines' compile caches (vLLM compiles
    /// for minutes in a fresh one), when the run names one.
    pub engine_cache: Option<PathBuf>,
}

/// The variable that names the directory the engines of inferd share their compile caches in (set
/// by the live scripts).
pub const ENGINE_CACHE_VAR: &str = "DOCKET_LIVE_ENGINE_CACHE";

impl Binaries {
    /// The binaries of a packaged run: the daemons beside `exe` (the program that runs them), and
    /// the siblings `build.rs` built into the target directory. `env` is the environment
    /// `main` hands in: [`ENGINE_CACHE_VAR`] names the engines' shared compile cache.
    pub fn beside(exe: &Path, env: &impl Fn(&str) -> Option<String>) -> Self {
        let beside = |name: &str| exe.with_file_name(name);
        Self {
            intentd: beside("accept-intentd"),
            companiond: beside("accept-companiond"),
            readerd: beside("accept-readerd"),
            memoryd: PathBuf::from(env!("ACCEPT_MEMORYD")),
            accountd: PathBuf::from(env!("ACCEPT_ACCOUNTD")),
            inferd: PathBuf::from(env!("ACCEPT_INFERD")),
            quire_do: beside("accept-quire-do"),
            actions_mcp: beside("accept-actions-mcp"),
            relay: beside("accept-bus-relay"),
            engine_cache: env(ENGINE_CACHE_VAR)
                .filter(|root| !root.is_empty())
                .map(PathBuf::from),
        }
    }
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
    /// `[agent.acp] agents = "on"`: intentd counts the host of an external agent
    /// (`org.quire.AcpAgent`) and `agents.toml` may start programs.
    Agents,
}

/// The scratch `settings.toml` of a world: the person's ACP setting.
pub fn settings_toml(acp: AcpSetting) -> String {
    match acp {
        AcpSetting::Off => String::new(),
        AcpSetting::On => "[agent.acp]\nexpose = \"on\"\n".to_owned(),
        AcpSetting::Agents => "[agent.acp]\nagents = \"on\"\n".to_owned(),
    }
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
    /// A durable home that already holds the cloud key (`--keys-home`): inferd's keys are the
    /// file in it and its non-secret records are copied into the scratch root, so no key is
    /// typed per world.
    pub keys_home: Option<crate::keys_home::KeysHome>,
    /// A catalogue directory whose `*.toml` entries are copied into the scratch root before
    /// inferd starts (see `catalog`).
    pub catalog: Option<PathBuf>,
    /// Whether inferd's replay engine writes down every request it is asked.
    pub record: Record,
    /// Whether the person switched the ACP edge on.
    pub acp: AcpSetting,
    /// What is open in the app's window, which "this thread" means.
    pub focus: Focus,
    /// The app the world serves.
    pub app: App,
}

/// Copies the run's catalogue into the scratch root; a missing source is a failed start.
pub fn copy_catalog(from: Option<&Path>, root: &Path) {
    if let Some(from) = from {
        crate::catalog::copy_entries(from, &crate::catalog::world_dir(root))
            .unwrap_or_else(|e| panic!("catalogue {}: {e}", from.display()));
    }
}

/// Whether inferd's replay engine records the request bodies it is asked, to
/// `<scratch>/record.jsonl`; a world prints them when it is dropped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Record {
    /// Nothing is recorded.
    #[default]
    Off,
    /// Every request body is.
    On,
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
    pub fn made(keep_in: Option<&Path>) -> std::io::Result<Self> {
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
    /// What the app was asked and what it holds.
    pub app: AppLog,
    /// A connection that owns `org.quire.Shell` and `org.quire.Confirm1`: sill.
    pub sill: zbus::Connection,
    _helpers: Vec<zbus::Connection>,
    /// The relays behind [`World::connect_as`]'s connections, ended with the world.
    relays: std::sync::Mutex<Vec<Reaped>>,
    /// The scratch root.
    pub dir: Scratch,
    bus: PrivateBus,
}

impl std::fmt::Debug for World {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("World")
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let text = self.recorded_requests();
        if !text.is_empty() {
            eprintln!("---- planner and reader requests (`Options::record`)\n{text}");
        }
    }
}
