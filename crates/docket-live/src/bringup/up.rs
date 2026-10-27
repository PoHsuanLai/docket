//! Starting the agent services on a bus that already exists: the same daemons, the same
//! environment and the same fake proc root as an acceptance `World`, but as processes that
//! outlive this one. Their pids and start times are recorded in the root for `down`.

use super::BringupError;
use super::cli::UpArgs;
use super::down;
use super::procs::{self, Record};
use super::refuse::{self, Own};
use super::seed;
use docket_accept::keys_home::KeysHome;
use docket_accept::world::{Binaries, Cgroup, InferdEnv, env_of, extra_env};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use zbus::fdo::DBusProxy;

/// How long a daemon gets to put its name on the bus.
const NAME_BOUND: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(100);

/// One daemon `up` started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    /// The daemon's name.
    pub name: &'static str,
    /// Its process id.
    pub pid: u32,
}

/// The daemons in start order, each with its binary and the bus name that says it is ready.
fn plan(binaries: &Binaries) -> [(&'static str, &Path, &'static str); 4] {
    [
        ("inferd", binaries.inferd.as_path(), "org.quire.Inference1"),
        ("memoryd", binaries.memoryd.as_path(), "org.quire.Memory1"),
        ("intentd", binaries.intentd.as_path(), "org.quire.Intents1"),
        (
            "companiond",
            binaries.companiond.as_path(),
            "org.quire.Companion1",
        ),
    ]
}

fn spawn(
    root: &Path,
    bus: &str,
    name: &'static str,
    binary: &Path,
    inferd: InferdEnv<'_>,
) -> std::io::Result<Child> {
    let log = std::fs::File::create(root.join("logs").join(format!("{name}.log")))?;
    let mut command = Command::new(binary);
    command
        .env_clear()
        .envs(env_of(root))
        .envs(extra_env(root, name, inferd))
        .env("DBUS_SESSION_BUS_ADDRESS", bus)
        .env("DBUS_SYSTEM_BUS_ADDRESS", bus)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log));
    // A group of its own: a Ctrl-C at the terminal that ran `up` does not reach it.
    command.process_group(0).spawn()
}

/// Waits until `name` has an owner on the bus, the child has not exited, and the bound is not up.
async fn until_owned(
    dbus: &DBusProxy<'_>,
    name: &'static str,
    owns: &str,
    child: &mut Child,
) -> Result<(), BringupError> {
    let bus_name = zbus::names::BusName::try_from(owns)
        .map_err(|e| BringupError::Bus(format!("{owns}: {e}")))?;
    let end = Instant::now() + NAME_BOUND;
    loop {
        if dbus.name_has_owner(bus_name.clone()).await.unwrap_or(false) {
            return Ok(());
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(BringupError::Exited(name, status.to_string()));
        }
        if Instant::now() >= end {
            return Err(BringupError::NoName(name, owns.to_owned()));
        }
        tokio::time::sleep(POLL).await;
    }
}

fn check_binaries(binaries: &Binaries) -> Result<(), BringupError> {
    for (_, path, _) in plan(binaries) {
        if !path.is_file() {
            return Err(BringupError::NoBinary(path.to_path_buf()));
        }
    }
    Ok(())
}

/// Starts inferd, memoryd, intentd and companiond on `args.bus`, rooted in `args.root`, and
/// waits for each one's bus name. On any failure everything started so far is stopped again.
pub async fn up(
    args: &UpArgs,
    binaries: &Binaries,
    own: &Own,
) -> Result<Vec<Started>, BringupError> {
    refuse::check_root(&args.root, own)?;
    refuse::check_bus(&args.bus, own)?;
    check_binaries(binaries)?;
    let root = args.root.as_path();
    let keys = args.keys_home.as_deref().map(KeysHome::open).transpose()?;
    let extra = match &args.inferd_config {
        Some(file) => std::fs::read_to_string(file)
            .map_err(|e| BringupError::Config(format!("{}: {e}", file.display())))?,
        None => String::new(),
    };
    let inferd_toml = seed::inferd_toml(root, &extra)?;
    let running = procs::read(root)?;
    if let Some(live) = running.iter().find(|r| r.state() == procs::State::Running) {
        return Err(BringupError::AlreadyUp(live.name.clone(), live.pid));
    }

    seed::layout(root)?;
    docket_accept::runlink::link_run(root)?;
    seed::files(root, args.shell_pid, &inferd_toml)?;
    if let Some(from) = &args.catalog {
        docket_accept::catalog::copy_entries(from, &docket_accept::catalog::world_dir(root))
            .map_err(|e| BringupError::Config(format!("catalogue {}: {e}", from.display())))?;
    }
    if let Some(from) = &args.providers {
        docket_accept::catalog::copy_entries(from, &root.join("data/porter/providers"))
            .map_err(|e| BringupError::Config(format!("providers {}: {e}", from.display())))?;
    }
    if let Some(home) = &keys {
        home.seed(root)?;
    }
    procs::write(root, &[])?;

    let connection = zbus::connection::Builder::address(args.bus.as_str())
        .map_err(|e| BringupError::Bus(e.to_string()))?
        .build()
        .await
        .map_err(|e| BringupError::Bus(e.to_string()))?;
    let dbus = DBusProxy::new(&connection)
        .await
        .map_err(|e| BringupError::Bus(e.to_string()))?;
    let keys_setting = keys.as_ref().map(KeysHome::keys_setting);
    let inferd = InferdEnv {
        keys: keys_setting.as_deref(),
        engine_cache: binaries.engine_cache.as_deref(),
    };

    let mut records: Vec<Record> = Vec::new();
    let mut started = Vec::new();
    for (name, binary, owns) in plan(binaries) {
        let step = start_one(
            root,
            &args.bus,
            (name, binary, owns),
            inferd,
            &dbus,
            &mut records,
        )
        .await;
        match step {
            Ok(pid) => started.push(Started { name, pid }),
            Err(why) => {
                for record in records.iter().rev() {
                    down::stop(record);
                }
                return Err(why);
            }
        }
    }
    Ok(started)
}

async fn start_one(
    root: &Path,
    bus: &str,
    (name, binary, owns): (&'static str, &Path, &'static str),
    inferd: InferdEnv<'_>,
    dbus: &DBusProxy<'_>,
    records: &mut Vec<Record>,
) -> Result<u32, BringupError> {
    let mut child = spawn(root, bus, name, binary, inferd)?;
    let pid = child.id();
    // Recorded before anything can fail, so a failure stops it too.
    let start = procs::start_time(pid).ok_or(BringupError::NoStartTime(name, pid))?;
    records.push(Record {
        name: name.to_owned(),
        pid,
        start,
    });
    procs::write(root, records)?;
    seed::place(root, pid, Cgroup::Unit(name))?;
    until_owned(dbus, name, owns, &mut child).await?;
    Ok(pid)
}
