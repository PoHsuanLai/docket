//! The command line of `docket-bringup`, parsed into a request. Pure: it reads no file and no
//! environment, so a table of command lines tests it.

use std::path::PathBuf;

/// What the person asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Start the agent services on a bus.
    Up(UpArgs),
    /// Stop the services a root recorded.
    Down(RootArgs),
    /// List the services a root recorded.
    Status(RootArgs),
}

/// `docket-bringup up`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpArgs {
    /// The private bus the services join (`DBUS_SESSION_BUS_ADDRESS` of the demo).
    pub bus: String,
    /// The scratch root: HOME, every XDG directory, the fake proc root, the logs.
    pub root: PathBuf,
    /// The shell's process, which the fake proc root puts in sill's unit.
    pub shell_pid: u32,
    /// A durable home that already holds the cloud key.
    pub keys_home: Option<PathBuf>,
    /// A catalogue directory whose `*.toml` entries are copied in.
    pub catalog: Option<PathBuf>,
    /// A directory of provider files (`*.toml`, no secrets) copied into `data/porter/providers`:
    /// `inferd add` and the daemon serve only the providers named there.
    pub providers: Option<PathBuf>,
    /// A file of extra `inferd.toml` rows (`[ai]` only: the person's data and model choices).
    pub inferd_config: Option<PathBuf>,
}

/// `docket-bringup down` and `status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootArgs {
    /// The scratch root `up` was given.
    pub root: PathBuf,
}

/// Why the command line is wrong.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum UsageError {
    /// No subcommand, or one that does not exist.
    #[error(
        "usage: docket-bringup up --bus ADDRESS --root DIR --shell-pid PID [--keys-home DIR] [--catalog DIR] [--providers DIR] [--inferd-config FILE]\n       docket-bringup down --root DIR\n       docket-bringup status --root DIR"
    )]
    Subcommand,
    /// A flag needs a value.
    #[error("{0} needs a value")]
    NoValue(String),
    /// A flag is not known to this subcommand.
    #[error("unknown option {0}")]
    Unknown(String),
    /// A required flag is missing.
    #[error("{0} is required")]
    Missing(&'static str),
    /// `--shell-pid` is not a process id.
    #[error("--shell-pid takes a process id (a whole number above 0), not {0:?}")]
    Pid(String),
    /// A path flag is relative.
    #[error("{0} {1} is not an absolute path")]
    Relative(&'static str, PathBuf),
}

const UP_FLAGS: [&str; 7] = [
    "--bus",
    "--root",
    "--shell-pid",
    "--keys-home",
    "--catalog",
    "--providers",
    "--inferd-config",
];
const ROOT_FLAGS: [&str; 1] = ["--root"];

fn absolute(flag: &'static str, path: PathBuf) -> Result<PathBuf, UsageError> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(UsageError::Relative(flag, path))
    }
}

/// Reads the arguments after the program's name.
pub fn parse(args: &[String]) -> Result<Command, UsageError> {
    let (sub, rest) = args.split_first().ok_or(UsageError::Subcommand)?;
    let allowed: &[&str] = match sub.as_str() {
        "up" => &UP_FLAGS,
        "down" | "status" => &ROOT_FLAGS,
        _ => return Err(UsageError::Subcommand),
    };
    let mut words = rest.iter();
    let (mut bus, mut root, mut shell) = (None, None, None);
    let (mut keys_home, mut catalog, mut inferd_config) = (None, None, None);
    let mut providers = None;
    while let Some(flag) = words.next() {
        if !allowed.contains(&flag.as_str()) {
            return Err(UsageError::Unknown(flag.clone()));
        }
        let value = words
            .next()
            .ok_or_else(|| UsageError::NoValue(flag.clone()))?;
        match flag.as_str() {
            "--bus" => bus = Some(value.clone()),
            "--root" => root = Some(PathBuf::from(value)),
            "--shell-pid" => shell = Some(value.clone()),
            "--keys-home" => keys_home = Some(PathBuf::from(value)),
            "--catalog" => catalog = Some(PathBuf::from(value)),
            "--providers" => providers = Some(PathBuf::from(value)),
            _ => inferd_config = Some(PathBuf::from(value)),
        }
    }
    let root = absolute("--root", root.ok_or(UsageError::Missing("--root"))?)?;
    if sub != "up" {
        let args = RootArgs { root };
        return Ok(match sub.as_str() {
            "down" => Command::Down(args),
            _ => Command::Status(args),
        });
    }
    let shell = shell.ok_or(UsageError::Missing("--shell-pid"))?;
    let shell_pid = shell
        .parse::<u32>()
        .ok()
        .filter(|pid| *pid > 0)
        .ok_or(UsageError::Pid(shell))?;
    let keep = |flag: &'static str, path: Option<PathBuf>| path.map(|p| absolute(flag, p));
    Ok(Command::Up(UpArgs {
        bus: bus.ok_or(UsageError::Missing("--bus"))?,
        root,
        shell_pid,
        keys_home: keep("--keys-home", keys_home).transpose()?,
        catalog: keep("--catalog", catalog).transpose()?,
        providers: keep("--providers", providers).transpose()?,
        inferd_config: keep("--inferd-config", inferd_config).transpose()?,
    }))
}
