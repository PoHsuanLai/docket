//! What a daemon's process is given: the scratch environment, and the few variables only one
//! daemon is told.

use docket_testbus::Reaped;
use std::path::Path;
use std::process::{Command, Stdio};

pub fn env_of(dir: &Path) -> Vec<(&'static str, String)> {
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

/// What only accountd and inferd are told besides the scratch root.
#[derive(Debug, Clone, Copy, Default)]
pub struct DaemonEnv<'a> {
    /// Where accountd's AI keys are (`ACCOUNTD_KEYS`), else a file in the scratch root.
    pub keys: Option<&'a str>,
    /// The directory its engines keep their compile caches in: a world is fresh each time, so
    /// without a shared one vLLM compiles for minutes in every world. inferd's engines inherit
    /// its environment.
    pub engine_cache: Option<&'a Path>,
}

fn engine_cache(root: Option<&Path>) -> Vec<(&'static str, String)> {
    root.map(|root| {
        let root = root.display();
        vec![
            ("VLLM_CACHE_ROOT", format!("{root}/vllm")),
            ("TRITON_CACHE_DIR", format!("{root}/triton")),
            ("TORCHINDUCTOR_CACHE_DIR", format!("{root}/inductor")),
        ]
    })
    .unwrap_or_default()
}

/// What only one daemon is told. memoryd's packaged binary runs a test build: its keys are a file
/// (there is no Secret Service on the private bus), its sandbox stays ON, and accountd, inferd and memoryd read
/// their callers from the scratch fake proc root instead of `/proc`.
pub fn extra_env(dir: &Path, name: &str, daemon: DaemonEnv<'_>) -> Vec<(&'static str, String)> {
    let proc_root = dir.join("proc").display().to_string();
    let system = |unit: &str| {
        dir.join("system")
            .join(format!("{unit}-callers.toml"))
            .display()
            .to_string()
    };
    match name {
        "memoryd" => vec![
            (
                "MEMORYD_KEYS",
                format!("file:{}", dir.join("keys/memoryd.keys").display()),
            ),
            ("MEMORYD_PROC_ROOT", proc_root),
            // The shell's `sheet_host` role is a fixed one: memoryd's table gives it from the
            // system's file alone, never the user's. memoryd reads `/etc/quire` for that file
            // until it honours this variable (a test build, with the fixture `/proc`).
            ("MEMORYD_SYSTEM_CALLERS", system("memory")),
        ],
        // accountd holds the AI keys: the home's key file for a cloud run, else a file in the
        // scratch root (a build with the `test-keys` feature; see docs/live-eval.md). Its Tailscale
        // socket is a scratch path, so it never reaches the real one.
        "accountd" => vec![
            ("ACCOUNTD_PROC_ROOT", proc_root),
            // Who is calling comes from this system file alone.
            ("PORTER_SYSTEM_CALLERS", system("accountd")),
            (
                "ACCOUNTD_TAILSCALE_SOCKET",
                dir.join("none.sock").display().to_string(),
            ),
            (
                "ACCOUNTD_KEYS",
                daemon.keys.map_or_else(
                    || format!("file:{}", dir.join("keys/accountd.keys").display()),
                    str::to_owned,
                ),
            ),
        ],
        // inferd asks accountd for the keys; its engines inherit its environment.
        "inferd" => [
            ("INFERD_PROC_ROOT", proc_root),
            // Who is calling comes from this system file alone (not `inferd.toml`).
            ("PORTER_INFERD_CALLERS", system("inferd")),
        ]
        .into_iter()
        .chain(engine_cache(daemon.engine_cache))
        .collect(),
        // intentd, companiond, readerd and docket-acp name their callers by the units and cgroups of
        // the scratch root's fake proc, and read the system's callers file from the scratch
        // root's own system directory.
        "intentd" => vec![
            (crate::seams::INTENTD_PROC_ROOT_VAR, proc_root),
            (crate::seams::INTENTD_SYSTEM_CALLERS_VAR, system("intentd")),
        ],
        "companiond" => vec![
            (crate::seams::COMPANIOND_PROC_ROOT_VAR, proc_root),
            (
                crate::seams::COMPANIOND_SYSTEM_CALLERS_VAR,
                system("companiond"),
            ),
        ],
        "readerd" => vec![
            (crate::seams::READERD_PROC_ROOT_VAR, proc_root),
            (crate::seams::READERD_SYSTEM_CALLERS_VAR, system("readerd")),
        ],
        "docket-acp" => vec![
            (crate::seams::ACP_PROC_ROOT_VAR, proc_root),
            (crate::seams::ACP_SYSTEM_CALLERS_VAR, system("acp")),
        ],
        _ => Vec::new(),
    }
}

/// Starts the daemon `name` from `binary` in the scratch root `dir`, on the bus at `bus`, with the
/// model tap on when `tap` names a file.
pub fn spawn(
    dir: &Path,
    bus: &str,
    name: &'static str,
    binary: &Path,
    tap: Option<&Path>,
    daemon: DaemonEnv<'_>,
) -> std::io::Result<(&'static str, Reaped)> {
    let log = std::fs::File::create(dir.join("logs").join(format!("{name}.log")))?;
    let mut command = Command::new(binary);
    command
        .env_clear()
        .envs(env_of(dir))
        .envs(extra_env(dir, name, daemon))
        .env("DBUS_SESSION_BUS_ADDRESS", bus)
        .env("DBUS_SYSTEM_BUS_ADDRESS", bus)
        .envs(tap.map(|file| (crate::seams::TRACE_VAR, file.to_owned())))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log));
    Ok((name, Reaped::spawn(&mut command)?))
}
