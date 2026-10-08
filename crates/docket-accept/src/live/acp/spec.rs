//! The external agent a run drives, as an `agents.toml` entry. It comes from the command line
//! and the harness writes it into the scratch configuration; nothing is read from the person's
//! own `agents.toml`. A path the agent keeps its state in is given relative to the scratch HOME,
//! so the entry can never name a place in the person's real one.

use docket_shell::NetworkMode;
use std::path::{Component, Path, PathBuf};

/// Where Claude Code keeps its login, relative to HOME.
pub const CLAUDE_CREDENTIALS_AT: &str = ".claude/.credentials.json";

/// Why a spec is not usable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SpecFault {
    /// The program's name is not one.
    #[error("--acp-program {0:?} is not a program name (lower-case letters, digits and -)")]
    Program(String),
    /// The command is not an absolute path.
    #[error("--acp-command must be an absolute path")]
    Command,
    /// A state or credentials path escapes the scratch HOME.
    #[error("{0:?}: a path inside the scratch HOME is relative and has no `..`")]
    Inside(String),
    /// A read-only path is not absolute.
    #[error("--acp-reads {0:?} must be an absolute path")]
    Reads(String),
    /// The network word is not one.
    #[error(
        "--acp-network takes none or host (the login route has no endpoint to be limited to), not {0:?}"
    )]
    Network(String),
    /// A route other than login.
    #[error(
        "--acp-route {0:?}: the harness has no accountd on its private bus, so only the login route can run"
    )]
    Route(String),
    /// A `--acp-profile` that is not a preset `agents.toml` knows.
    #[error("--acp-profile takes claude-code or agy, not {0:?}")]
    Profile(String),
    /// A `--acp-sign-in` that is not a short id.
    #[error("--acp-sign-in takes 1 to 64 letters, digits, - _ or ., not {0:?}")]
    SignIn(String),
    /// A `--acp-set` that is not NAME=VALUE.
    #[error("--acp-set takes NAME=VALUE, not {0:?}")]
    Set(String),
}

/// A login to copy into the scratch HOME for the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialsSource {
    /// The file to copy, from the flag. Never a default.
    pub from: PathBuf,
    /// Where it goes, relative to the scratch HOME.
    pub at: String,
}

/// One agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpSpec {
    /// The program's name (`agents.toml`'s `program`).
    pub program: String,
    /// The executable, absolute.
    pub command: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// The network its sandbox gets. `host` for an agent that must reach its provider.
    pub network: NetworkMode,
    /// Paths it keeps its login and settings in, relative to the scratch HOME.
    pub state: Vec<String>,
    /// Read-only paths (where the program is installed), absolute.
    pub reads: Vec<PathBuf>,
    /// Plain environment variables.
    pub set: Vec<(String, String)>,
    /// The login to stage, if the agent signs in with one.
    pub credentials: Option<CredentialsSource>,
    /// The `agents.toml` preset that confines the agent's own extras (`profile`), if any.
    pub profile: Option<String>,
    /// The way the agent signs itself in at start (`sign_in`), one of the ids it advertises.
    pub sign_in: Option<String>,
}

impl AcpSpec {
    /// A spec that runs `command` as `program` with the network of a login agent.
    pub fn new(program: &str, command: PathBuf) -> Self {
        Self {
            program: program.to_owned(),
            command,
            args: Vec::new(),
            network: NetworkMode::Host,
            state: Vec::new(),
            reads: Vec::new(),
            set: Vec::new(),
            credentials: None,
            profile: None,
            sign_in: None,
        }
    }

    /// Checks everything the command line can get wrong.
    pub fn check(&self) -> Result<(), SpecFault> {
        let name_ok = !self.program.is_empty()
            && self
                .program
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !name_ok {
            return Err(SpecFault::Program(self.program.clone()));
        }
        if !self.command.is_absolute() {
            return Err(SpecFault::Command);
        }
        if let Some(bad) = self.reads.iter().find(|p| !p.is_absolute()) {
            return Err(SpecFault::Reads(bad.display().to_string()));
        }
        if let Some(profile) = self
            .profile
            .as_ref()
            .filter(|p| !matches!(p.as_str(), "claude-code" | "agy"))
        {
            return Err(SpecFault::Profile(profile.clone()));
        }
        if let Some(method) = self
            .sign_in
            .as_ref()
            .filter(|m| docket_acp::client::SignIn::parse(m).is_err())
        {
            return Err(SpecFault::SignIn(method.clone()));
        }
        let credentials = self.credentials.iter().map(|c| c.at.as_str());
        for inside in self.state.iter().map(String::as_str).chain(credentials) {
            if !stays_inside(inside) {
                return Err(SpecFault::Inside(inside.to_owned()));
            }
        }
        Ok(())
    }

    /// The `agents.toml` text for this agent working with `home` as its HOME.
    pub fn entry_toml(&self, home: &Path) -> String {
        let quote = |text: &str| toml::Value::String(text.to_owned()).to_string();
        let list = |items: Vec<String>| format!("[{}]", items.join(", "));
        let path = |p: &Path| quote(&p.display().to_string());
        let mut out = String::from("[[agent]]\n");
        out.push_str(&format!("program = {}\n", quote(&self.program)));
        out.push_str(&format!("command = {}\n", path(&self.command)));
        out.push_str(&format!(
            "args = {}\n",
            list(self.args.iter().map(|a| quote(a)).collect())
        ));
        out.push_str("route = \"login\"\n");
        out.push_str(&format!(
            "network = {}\n",
            quote(network_word(self.network))
        ));
        out.push_str(&format!(
            "reads = {}\n",
            list(self.reads.iter().map(|p| path(p)).collect())
        ));
        out.push_str(&format!(
            "state = {}\n",
            list(self.state.iter().map(|s| path(&home.join(s))).collect())
        ));
        out.push_str(&format!("home = {}\n", path(home)));
        out.push_str("tools = \"offered\"\n");
        if let Some(profile) = &self.profile {
            out.push_str(&format!("profile = {}\n", quote(profile)));
        }
        if let Some(method) = &self.sign_in {
            out.push_str(&format!("sign_in = {}\n", quote(method)));
        }
        if !self.set.is_empty() {
            out.push_str("[agent.set]\n");
            for (name, value) in &self.set {
                out.push_str(&format!("{name} = {}\n", quote(value)));
            }
        }
        out
    }
}

/// A relative path without `..` or a root: one that stays inside what it is joined to.
fn stays_inside(text: &str) -> bool {
    let path = Path::new(text);
    !text.is_empty()
        && path.is_relative()
        && path
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// The word `agents.toml` and the flag use for a network.
pub fn network_word(mode: NetworkMode) -> &'static str {
    match mode {
        NetworkMode::None => "none",
        NetworkMode::EndpointOnly => "endpoint_only",
        NetworkMode::Host => "host",
    }
}

/// The network a flag names.
pub fn network_of(word: &str) -> Result<NetworkMode, SpecFault> {
    match word {
        "none" => Ok(NetworkMode::None),
        "host" => Ok(NetworkMode::Host),
        other => Err(SpecFault::Network(other.to_owned())),
    }
}
