//! The command line of `docket-live`, parsed into a request. Pure: it reads no file and no
//! environment, so a table of command lines tests it.

use crate::live::acp::AcpSpec;
use crate::live::acp::spec::{
    CLAUDE_CREDENTIALS_AT, CredentialsSource, RegistryPick, SpecFault, Task, network_of,
};
use crate::live::engine::{Engine, EngineError};
use docket_core::ShadowMode;
use std::path::PathBuf;

/// Who plays the companion's part in a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Agent {
    /// The companion's own planner, over the model of `--engine`.
    Planner,
    /// An external ACP agent, hosted as `docket-agent` hosts it. `--engine` still says where the
    /// policy writer and the reviewers get their answers.
    Acp(Box<AcpSpec>),
}

/// What the person asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Play the eval corpora (or the regression pairs) through the router over a model.
    Corpus(CorpusArgs),
    /// Play the acceptance flows against a model.
    Smoke(SmokeArgs),
}

/// `docket-live corpus`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusArgs {
    /// Where the model answers come from.
    pub engine: Engine,
    /// The owner's `inferd.toml`.
    pub inferd_config: Option<PathBuf>,
    /// The report's name.
    pub label: String,
    /// Where traces and scratch go.
    pub out: PathBuf,
    /// Where the markdown report is written.
    pub report: Option<PathBuf>,
    /// The `eval/` directory.
    pub eval_dir: PathBuf,
    /// Only these corpora (a directory name of `eval/`); every one when empty.
    pub corpora: Vec<String>,
    /// Only these case ids; every one when empty.
    pub cases: Vec<String>,
    /// Play the regression pairs of this directory instead.
    pub regress: Option<PathBuf>,
    /// One bound in milliseconds for every review stage; the shipped ones when absent.
    pub timeout_ms: Option<u32>,
    /// The accountd binary of a cloud run.
    pub accountd: Option<PathBuf>,
    /// The durable directory that holds the cloud account (`dev/live/cloud-key.sh` fills it).
    pub accountd_home: Option<PathBuf>,
    /// Fail when a corpus's false-negative rate (the Wilson upper end) exceeds this, in
    /// thousandths.
    pub fnr_max_permille: Option<u32>,
    /// Seconds to wait for each model's warm-up (live engines).
    pub patience_s: u64,
    /// The catalogue directory to copy into the world.
    pub catalog: Option<PathBuf>,
    /// Who plays the companion's part.
    pub agent: Agent,
    /// Whether to run the shadow flagger beside the quick judge and report it.
    pub shadow: ShadowMode,
}

/// `docket-live smoke`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmokeArgs {
    /// Where the model answers come from.
    pub engine: Engine,
    /// The owner's `inferd.toml`.
    pub inferd_config: Option<PathBuf>,
    /// Where traces and scratch go.
    pub out: PathBuf,
    /// The `eval/` directory (its `hostile-model/planner` cases are played after the flows).
    pub eval_dir: PathBuf,
    /// Only these flows or planner cases; every one when empty.
    pub flows: Vec<String>,
    /// Seconds to wait for each change of an answer, and for each model's warm-up.
    pub patience_s: u64,
    /// The catalogue directory to copy into the world.
    pub catalog: Option<PathBuf>,
    /// Who plays the companion's part.
    pub agent: Agent,
    /// The accountd binary of a cloud run.
    pub accountd: Option<PathBuf>,
    /// The durable directory that holds the cloud account.
    pub accountd_home: Option<PathBuf>,
    /// How many times each flow is played (at least 1).
    pub repeat: u32,
}

/// Why the command line is wrong.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum UsageError {
    /// No subcommand, or one that does not exist.
    #[error("usage: docket-live corpus|smoke --engine scripted|local|cloud [options]")]
    Subcommand,
    /// A flag needs a value.
    #[error("{0} needs a value")]
    NoValue(String),
    /// A flag is not known.
    #[error("unknown option {0}")]
    Unknown(String),
    /// A number is not one.
    #[error("{0} takes a whole number, not {1:?}")]
    Number(String, String),
    /// `--engine` is missing or wrong.
    #[error("{0}")]
    Engine(String),
    /// `--agent` is not planner or acp.
    #[error("--agent takes planner or acp, not {0:?}")]
    Agent(String),
    /// The external agent's options are wrong or incomplete.
    #[error("{0}")]
    Acp(String),
    /// `--accountd-home` is relative, or has no `--accountd` to go with.
    #[error("{0}")]
    Home(String),
}

impl From<SpecFault> for UsageError {
    fn from(fault: SpecFault) -> Self {
        UsageError::Acp(fault.to_string())
    }
}

struct Words<'a>(std::slice::Iter<'a, String>);

impl<'a> Words<'a> {
    fn value(&mut self, flag: &str) -> Result<&'a String, UsageError> {
        self.0
            .next()
            .ok_or_else(|| UsageError::NoValue(flag.to_owned()))
    }
}

fn number<T: std::str::FromStr>(flag: &str, text: &str) -> Result<T, UsageError> {
    text.parse()
        .map_err(|_| UsageError::Number(flag.to_owned(), text.to_owned()))
}

fn engine(text: Option<String>) -> Result<Engine, UsageError> {
    let text = text.ok_or_else(|| UsageError::Engine("--engine is required".to_owned()))?;
    Engine::parse(&text).map_err(|e: EngineError| UsageError::Engine(e.to_string()))
}

/// Reads the arguments after the program's name.
pub fn parse(args: &[String]) -> Result<Command, UsageError> {
    let (sub, rest) = args.split_first().ok_or(UsageError::Subcommand)?;
    let mut words = Words(rest.iter());
    let mut engine_text = None;
    let mut config = None;
    let mut out = None;
    let (mut label, mut report, mut eval_dir, mut regress) = (None, None, None, None);
    let (mut corpora, mut cases, mut flows) = (Vec::new(), Vec::new(), Vec::new());
    let mut catalog = None;
    let (mut timeout, mut accountd, mut fnr, mut patience) = (None, None, None, 600_u64);
    let mut accountd_home: Option<PathBuf> = None;
    let mut repeat: Option<u32> = None;
    let mut agent_word = None;
    let mut shadow = ShadowMode::Off;
    let mut acp = AcpFlags::default();
    while let Some(flag) = words.0.next() {
        match flag.as_str() {
            "--agent" => agent_word = Some(words.value(flag)?.clone()),
            other if other.starts_with("--acp-") => acp.take(other, &mut words)?,
            "--engine" => engine_text = Some(words.value(flag)?.clone()),
            "--inferd-config" => config = Some(PathBuf::from(words.value(flag)?)),
            "--out" => out = Some(PathBuf::from(words.value(flag)?)),
            "--label" => label = Some(words.value(flag)?.clone()),
            "--report" => report = Some(PathBuf::from(words.value(flag)?)),
            "--eval-dir" => eval_dir = Some(PathBuf::from(words.value(flag)?)),
            "--regress" => regress = Some(PathBuf::from(words.value(flag)?)),
            "--corpus" => corpora.push(words.value(flag)?.clone()),
            "--case" => cases.push(words.value(flag)?.clone()),
            "--flow" => flows.push(words.value(flag)?.clone()),
            "--timeout-ms" => timeout = Some(number(flag, words.value(flag)?)?),
            "--catalog" => catalog = Some(PathBuf::from(words.value(flag)?)),
            "--accountd" => accountd = Some(PathBuf::from(words.value(flag)?)),
            "--accountd-home" => accountd_home = Some(PathBuf::from(words.value(flag)?)),
            "--repeat" => repeat = Some(number(flag, words.value(flag)?)?),
            "--fnr-max-permille" => fnr = Some(number(flag, words.value(flag)?)?),
            "--shadow" => shadow = ShadowMode::Record,
            "--patience-s" => patience = number(flag, words.value(flag)?)?,
            other => return Err(UsageError::Unknown(other.to_owned())),
        }
    }
    let engine = engine(engine_text)?;
    match (&accountd_home, &accountd) {
        (Some(home), _) if !home.is_absolute() => {
            return Err(UsageError::Home(format!(
                "--accountd-home {} is not an absolute path",
                home.display()
            )));
        }
        (Some(_), None) => {
            return Err(UsageError::Home(
                "--accountd-home needs --accountd (the test build of accountd)".to_owned(),
            ));
        }
        _ => {}
    }
    if repeat == Some(0) {
        return Err(UsageError::Home(
            "--repeat takes a number of at least 1".to_owned(),
        ));
    }
    if sub == "smoke" && accountd.is_some() && accountd_home.is_none() {
        return Err(UsageError::Home(
            "a smoke run starts a fresh accountd per flow, so --accountd needs --accountd-home"
                .to_owned(),
        ));
    }
    if sub == "corpus" && repeat.is_some() {
        return Err(UsageError::Unknown("--repeat (smoke only)".to_owned()));
    }
    let agent = acp.agent(agent_word)?;
    let out = out.unwrap_or_else(|| PathBuf::from("docket-live-out"));
    match sub.as_str() {
        "corpus" => Ok(Command::Corpus(CorpusArgs {
            engine,
            inferd_config: config,
            label: label.unwrap_or_else(|| "live".to_owned()),
            out,
            report,
            eval_dir: eval_dir.unwrap_or_else(|| PathBuf::from("eval")),
            corpora,
            cases,
            regress,
            timeout_ms: timeout,
            accountd,
            accountd_home,
            fnr_max_permille: fnr,
            patience_s: patience,
            catalog,
            agent,
            shadow,
        })),
        "smoke" => Ok(Command::Smoke(SmokeArgs {
            engine,
            inferd_config: config,
            out,
            eval_dir: eval_dir.unwrap_or_else(|| PathBuf::from("eval")),
            flows,
            patience_s: patience,
            catalog,
            agent,
            accountd,
            accountd_home,
            repeat: repeat.unwrap_or(1),
        })),
        _ => Err(UsageError::Subcommand),
    }
}

/// The `--acp-*` options as they come, before they are known to be wanted.
#[derive(Default)]
struct AcpFlags {
    program: Option<String>,
    command: Option<PathBuf>,
    args: Vec<String>,
    network: Option<String>,
    state: Vec<String>,
    reads: Vec<PathBuf>,
    set: Vec<String>,
    credentials: Option<PathBuf>,
    credentials_at: Option<String>,
    route: Option<String>,
    profile: Option<String>,
    sign_in: Option<String>,
    agent: Option<String>,
    version: Option<String>,
    agents_dir: Option<PathBuf>,
    model: Option<String>,
    list_models: Task,
    seen: bool,
}

impl AcpFlags {
    fn take(&mut self, flag: &str, words: &mut Words<'_>) -> Result<(), UsageError> {
        self.seen = true;
        if flag == "--acp-list-models" {
            self.list_models = Task::ListModels;
            return Ok(());
        }
        let value = words.value(flag)?.clone();
        match flag {
            "--acp-program" => self.program = Some(value),
            "--acp-command" => self.command = Some(PathBuf::from(value)),
            "--acp-arg" => self.args.push(value),
            "--acp-network" => self.network = Some(value),
            "--acp-state" => self.state.push(value),
            "--acp-reads" => self.reads.push(PathBuf::from(value)),
            "--acp-set" => self.set.push(value),
            "--acp-credentials" => self.credentials = Some(PathBuf::from(value)),
            "--acp-credentials-at" => self.credentials_at = Some(value),
            "--acp-route" => self.route = Some(value),
            "--acp-profile" => self.profile = Some(value),
            "--acp-sign-in" => self.sign_in = Some(value),
            "--acp-agent" => self.agent = Some(value),
            "--acp-version" => self.version = Some(value),
            "--acp-agents-dir" => self.agents_dir = Some(PathBuf::from(value)),
            "--acp-model" => self.model = Some(value),
            other => return Err(UsageError::Unknown(other.to_owned())),
        }
        Ok(())
    }

    /// The agent the options ask for: the planner unless `--agent acp`, and then the spec,
    /// checked. `--acp-*` options without `--agent acp` are a mistake, not an ignored line.
    fn agent(self, word: Option<String>) -> Result<Agent, UsageError> {
        match word.as_deref() {
            None | Some("planner") if !self.seen => Ok(Agent::Planner),
            None | Some("planner") => Err(UsageError::Acp(
                "--acp-* options need --agent acp".to_owned(),
            )),
            Some("acp") => self.spec().map(|spec| Agent::Acp(Box::new(spec))),
            Some(other) => Err(UsageError::Agent(other.to_owned())),
        }
    }

    fn spec(self) -> Result<AcpSpec, UsageError> {
        let need = |what: &str| UsageError::Acp(format!("--agent acp needs {what}"));
        let (command, registry) = match (self.command, self.agent) {
            (Some(command), None) => (command, None),
            (None, Some(id)) => {
                let version = self
                    .version
                    .ok_or_else(|| need("--acp-version with --acp-agent"))?;
                let dir = self
                    .agents_dir
                    .ok_or_else(|| need("--acp-agents-dir with --acp-agent"))?;
                (PathBuf::new(), Some(RegistryPick { id, version, dir }))
            }
            _ => return Err(need("--acp-command <program>, or --acp-agent (not both)")),
        };
        if let Some(route) = self.route.filter(|r| r != "login") {
            return Err(SpecFault::Route(route).into());
        }
        let fallback = registry.as_ref().map_or("claude-code", |p| p.id.as_str());
        let program = self.program.unwrap_or_else(|| fallback.to_owned());
        let mut spec = AcpSpec::new(&program, command);
        spec.registry = registry;
        spec.model = self.model;
        spec.task = self.list_models;
        spec.args = self.args;
        spec.state = self.state;
        spec.reads = self.reads;
        spec.profile = self.profile;
        spec.sign_in = self.sign_in;
        if let Some(word) = self.network {
            spec.network = network_of(&word)?;
        }
        for pair in self.set {
            let (name, value) = pair
                .split_once('=')
                .ok_or_else(|| SpecFault::Set(pair.clone()))?;
            spec.set.push((name.to_owned(), value.to_owned()));
        }
        spec.credentials = self.credentials.map(|from| CredentialsSource {
            from,
            at: self
                .credentials_at
                .unwrap_or_else(|| CLAUDE_CREDENTIALS_AT.to_owned()),
        });
        spec.check()?;
        Ok(spec)
    }
}
