//! The files docket writes for one run to confine what a program brings of its own, and the
//! variables that point the program at them. A preset (`profile` in `agents.toml`) names both;
//! the router never knows which program it is.
//!
//! The file lives in a directory of its own under the run directory (0700), is written 0400, and
//! is bound read-only into the sandbox last. It is never put under a path the agent can write:
//! its state, its working directory. The agent cannot widen what it allows, whatever it writes.
//!
//! Claude Code: the managed-settings file outranks the user's, the project's and the local
//! settings (Claude Code's own precedence; docket relies on it and tests only that the file and
//! the variable are in the spawn plan). It turns off the account's connectors, skills and
//! plugins, allows the desktop's tool server (and no other) to be called without Claude Code's
//! own prompt, so the router's sheet is the only one, and lets only that server be configured.

use crate::config::{Entry, Profile};
use docket_acp::client::SERVER_NAME;
use docket_core::AbsPath;
use porter_core::LauncherSession;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

/// The settings file's name inside the run's directory.
const SETTINGS_FILE: &str = "managed-settings.json";

/// The variable that names Claude Code's managed-settings file.
pub const SETTINGS_PATH_ENV: &str = "CLAUDE_CODE_MANAGED_SETTINGS_PATH";

/// Why the files could not be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ManagedFault {
    /// The place is not made, not writable, or not a path.
    #[error("the managed settings could not be written")]
    Io,
    /// The place is under a path the agent can write, where it could be shadowed or replaced.
    #[error("the managed settings would sit under a path the agent can write")]
    Writable,
}

/// What was written for one run.
#[derive(Debug)]
pub struct Managed {
    /// The directory, to remove with the session.
    pub dir: PathBuf,
    /// The file, to bind read-only and to name in the environment.
    pub file: AbsPath,
}

impl Profile {
    /// The settings file's text.
    pub fn settings(self) -> String {
        match self {
            Profile::ClaudeCode => claude_code_settings(),
        }
    }

    /// The variables that make the program read `file` and keep to it.
    pub fn env(self, file: &AbsPath) -> Vec<(&'static str, String)> {
        match self {
            Profile::ClaudeCode => vec![
                (SETTINGS_PATH_ENV, file.as_str().to_owned()),
                ("CLAUDE_CODE_DISABLE_CLAUDE_MDS", "1".to_owned()),
                ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1".to_owned()),
                ("ENABLE_CLAUDEAI_MCP_SERVERS", "false".to_owned()),
                ("DISABLE_AUTOUPDATER", "1".to_owned()),
            ],
        }
    }
}

/// The allow rule for every tool of the desktop's server and no other server.
pub fn allow_rule() -> String {
    format!("mcp__{SERVER_NAME}")
}

fn claude_code_settings() -> String {
    // `SERVER_NAME` is a fixed lower-case word, so it needs no escaping.
    format!(
        concat!(
            "{{\"disableClaudeAiConnectors\":true,",
            "\"syncClaudeAiSkills\":false,",
            "\"syncClaudeAiPlugins\":false,",
            "\"permissions\":{{\"allow\":[\"{rule}\"]}},",
            "\"allowManagedMcpServersOnly\":true,",
            "\"allowedMcpServers\":[{{\"serverName\":\"{server}\"}}]}}"
        ),
        rule = allow_rule(),
        server = SERVER_NAME
    )
}

fn under(path: &Path, roots: impl Iterator<Item = impl AsRef<Path>>) -> bool {
    roots.into_iter().any(|r| path.starts_with(r.as_ref()))
}

impl Managed {
    /// Writes the preset's file for `session` under `run_dir`, refusing a place under the
    /// entry's state or the working directory.
    pub fn write(
        profile: Profile,
        run_dir: &Path,
        session: &LauncherSession,
        entry: &Entry,
        cwd: &AbsPath,
    ) -> Result<Self, ManagedFault> {
        let dir = run_dir.join(format!("docket-managed-{}", session.as_str()));
        let writable = entry
            .state
            .iter()
            .map(|p| PathBuf::from(p.as_str()))
            .chain([PathBuf::from(cwd.as_str())]);
        if under(&dir, writable) {
            return Err(ManagedFault::Writable);
        }
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .map_err(|_| ManagedFault::Io)?;
        let written = write_file(&dir.join(SETTINGS_FILE), &profile.settings());
        let file = written.and_then(|path| {
            path.to_str()
                .and_then(|t| AbsPath::parse(t).ok())
                .ok_or(ManagedFault::Io)
        });
        match file {
            Ok(file) => Ok(Self { dir, file }),
            Err(fault) => {
                let _ = std::fs::remove_dir_all(&dir);
                Err(fault)
            }
        }
    }
}

fn write_file(path: &Path, text: &str) -> Result<PathBuf, ManagedFault> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o400)
        .open(path)
        .map_err(|_| ManagedFault::Io)?;
    file.write_all(text.as_bytes())
        .map_err(|_| ManagedFault::Io)?;
    Ok(path.to_owned())
}
