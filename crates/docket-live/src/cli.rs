//! The command line of `docket-live`, parsed into a request. Pure: it reads no file and no
//! environment, so a table of command lines tests it.

use crate::acp::AcpSpec;
use crate::acp::spec::{
    CLAUDE_CREDENTIALS_AT, CredentialsSource, RegistryPick, SpecFault, Task, network_of,
};
use crate::engine::{Engine, EngineError};
use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::{ArgAction, Args, Parser};
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

/// Who chooses the steps of an attack run (`--planner`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PlannerMode {
    /// The attack's scripted steps: a hijacked agent that follows every injection, the worst
    /// case the router has to hold against. The default, and the gate's.
    #[default]
    Scripted,
    /// The companion's real planner, given the person's words over the attack's world: it chooses
    /// its own steps, and the run is scored on what it did.
    Live,
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
    /// The durable directory that holds the cloud key (`dev/live/cloud-key.sh` fills it).
    pub keys_home: Option<PathBuf>,
    /// Fail when a corpus's false-negative rate (the Wilson upper end) exceeds this, in
    /// thousandths.
    pub fnr_max_permille: Option<u32>,
    /// Seconds to wait for each model's warm-up (live engines).
    pub patience_s: u64,
    /// The catalogue directory to copy into the world.
    pub catalog: Option<PathBuf>,
    /// Who plays the companion's part.
    pub agent: Agent,
    /// Where to write the attack suite's per-commit JSON summary (`eval/results`), when the run
    /// played the attacks (`--corpus attacks`).
    pub attack_summary: Option<PathBuf>,
    /// Who chooses the steps of the attacks (`--corpus attacks`).
    pub planner: PlannerMode,
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
    /// The durable directory that holds the cloud key.
    pub keys_home: Option<PathBuf>,
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
    /// `--keys-home` is relative, `--repeat` is zero, or a flag that moved was used.
    #[error("{0}")]
    Home(String),
    /// `--planner` is not scripted or live, or does not go with the rest of the line.
    #[error("{0}")]
    Planner(String),
}

impl From<SpecFault> for UsageError {
    fn from(fault: SpecFault) -> Self {
        UsageError::Acp(fault.to_string())
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

/// The command line as clap reads it: every option takes a value (the next word, even one that
/// starts with a dash) except `--acp-list-models`. Numbers stay words here; `number` reads them,
/// so a bad one says which flag it was.
#[derive(Parser)]
#[command(
    no_binary_name = true,
    disable_help_flag = true,
    disable_version_flag = true
)]
struct Raw {
    /// `corpus` or `smoke`.
    subcommand: Option<String>,
    #[arg(long = "agent", allow_hyphen_values = true)]
    agent_word: Option<String>,
    #[command(flatten)]
    acp: AcpFlags,
    #[arg(long, allow_hyphen_values = true)]
    engine: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    inferd_config: Option<PathBuf>,
    #[arg(long, allow_hyphen_values = true)]
    out: Option<PathBuf>,
    #[arg(long, allow_hyphen_values = true)]
    label: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    report: Option<PathBuf>,
    #[arg(long, allow_hyphen_values = true)]
    eval_dir: Option<PathBuf>,
    #[arg(long, allow_hyphen_values = true)]
    regress: Option<PathBuf>,
    #[arg(long = "corpus", allow_hyphen_values = true)]
    corpora: Vec<String>,
    #[arg(long = "case", allow_hyphen_values = true)]
    cases: Vec<String>,
    #[arg(long = "flow", allow_hyphen_values = true)]
    flows: Vec<String>,
    #[arg(long, allow_hyphen_values = true)]
    timeout_ms: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    catalog: Option<PathBuf>,
    #[arg(long, allow_hyphen_values = true)]
    keys_home: Option<PathBuf>,
    /// Gone with accountd; kept to say where it went.
    #[arg(long, hide = true, allow_hyphen_values = true)]
    accountd: Option<String>,
    /// Gone with accountd; kept to say where it went.
    #[arg(long, hide = true, allow_hyphen_values = true)]
    accountd_home: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    repeat: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    fnr_max_permille: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    attack_summary: Option<PathBuf>,
    #[arg(long, allow_hyphen_values = true)]
    planner: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    patience_s: Option<String>,
}

/// What clap refused, as this command line's own errors.
fn usage_of(error: &clap::Error) -> UsageError {
    let arg = match error.get(ContextKind::InvalidArg) {
        Some(ContextValue::String(arg)) => arg.as_str(),
        _ => "",
    };
    match error.kind() {
        ErrorKind::InvalidValue => {
            UsageError::NoValue(arg.split_whitespace().next().unwrap_or_default().to_owned())
        }
        ErrorKind::UnknownArgument => UsageError::Unknown(arg.to_owned()),
        _ => UsageError::Unknown(error.to_string()),
    }
}

/// Reads the arguments after the program's name.
pub fn parse(args: &[String]) -> Result<Command, UsageError> {
    let raw = Raw::try_parse_from(args).map_err(|e| usage_of(&e))?;
    let sub = raw.subcommand.ok_or(UsageError::Subcommand)?;
    if let Some(moved) = [
        ("--accountd", &raw.accountd),
        ("--accountd-home", &raw.accountd_home),
    ]
    .into_iter()
    .find_map(|(flag, value)| value.as_ref().map(|_| flag))
    {
        return Err(UsageError::Home(format!(
            "{moved} is gone: inferd holds the AI keys now, not accountd. Use --keys-home DIR (dev/live/cloud-key.sh fills it)"
        )));
    }
    let timeout = raw
        .timeout_ms
        .as_deref()
        .map(|t| number("--timeout-ms", t))
        .transpose()?;
    let repeat: Option<u32> = raw
        .repeat
        .as_deref()
        .map(|t| number("--repeat", t))
        .transpose()?;
    let fnr = raw
        .fnr_max_permille
        .as_deref()
        .map(|t| number("--fnr-max-permille", t))
        .transpose()?;
    let patience = raw
        .patience_s
        .as_deref()
        .map_or(Ok(600_u64), |t| number("--patience-s", t))?;
    let engine = engine(raw.engine)?;
    let keys_home = raw.keys_home;
    if let Some(home) = keys_home.as_ref().filter(|home| !home.is_absolute()) {
        return Err(UsageError::Home(format!(
            "--keys-home {} is not an absolute path",
            home.display()
        )));
    }
    if repeat == Some(0) {
        return Err(UsageError::Home(
            "--repeat takes a number of at least 1".to_owned(),
        ));
    }
    if sub == "corpus" && repeat.is_some() {
        return Err(UsageError::Unknown("--repeat (smoke only)".to_owned()));
    }
    if sub == "smoke" && raw.planner.is_some() {
        return Err(UsageError::Unknown("--planner (corpus only)".to_owned()));
    }
    let planner = match raw.planner.as_deref() {
        None | Some("scripted") => PlannerMode::Scripted,
        Some("live") => PlannerMode::Live,
        Some(other) => {
            return Err(UsageError::Planner(format!(
                "--planner takes scripted or live, not {other:?}"
            )));
        }
    };
    let agent = raw.acp.agent(raw.agent_word)?;
    if planner == PlannerMode::Live && matches!(agent, Agent::Acp(_)) {
        return Err(UsageError::Planner(
            "--planner live plays the companion's own planner; it does not go with --agent acp"
                .to_owned(),
        ));
    }
    if planner == PlannerMode::Live && engine == Engine::Scripted {
        return Err(UsageError::Planner(
            "--planner live needs a live engine (--engine local or cloud): a cassette has no planner to play"
                .to_owned(),
        ));
    }
    let out = raw.out.unwrap_or_else(|| PathBuf::from("docket-live-out"));
    match sub.as_str() {
        "corpus" => Ok(Command::Corpus(CorpusArgs {
            engine,
            inferd_config: raw.inferd_config,
            label: raw.label.unwrap_or_else(|| "live".to_owned()),
            out,
            report: raw.report,
            eval_dir: raw.eval_dir.unwrap_or_else(|| PathBuf::from("eval")),
            corpora: raw.corpora,
            cases: raw.cases,
            regress: raw.regress,
            timeout_ms: timeout,
            keys_home,
            fnr_max_permille: fnr,
            patience_s: patience,
            catalog: raw.catalog,
            agent,
            attack_summary: raw.attack_summary,
            planner,
        })),
        "smoke" => Ok(Command::Smoke(SmokeArgs {
            engine,
            inferd_config: raw.inferd_config,
            out,
            eval_dir: raw.eval_dir.unwrap_or_else(|| PathBuf::from("eval")),
            flows: raw.flows,
            patience_s: patience,
            catalog: raw.catalog,
            agent,
            keys_home,
            repeat: repeat.unwrap_or(1),
        })),
        _ => Err(UsageError::Subcommand),
    }
}

/// The `--acp-*` options as they come, before they are known to be wanted.
#[derive(Args, Default, PartialEq)]
struct AcpFlags {
    #[arg(long = "acp-program", allow_hyphen_values = true)]
    program: Option<String>,
    #[arg(long = "acp-command", allow_hyphen_values = true)]
    command: Option<PathBuf>,
    #[arg(long = "acp-arg", allow_hyphen_values = true)]
    args: Vec<String>,
    #[arg(long = "acp-network", allow_hyphen_values = true)]
    network: Option<String>,
    #[arg(long = "acp-state", allow_hyphen_values = true)]
    state: Vec<String>,
    #[arg(long = "acp-reads", allow_hyphen_values = true)]
    reads: Vec<PathBuf>,
    #[arg(long = "acp-set", allow_hyphen_values = true)]
    set: Vec<String>,
    #[arg(long = "acp-credentials", allow_hyphen_values = true)]
    credentials: Option<PathBuf>,
    #[arg(long = "acp-credentials-at", allow_hyphen_values = true)]
    credentials_at: Option<String>,
    #[arg(long = "acp-route", allow_hyphen_values = true)]
    route: Option<String>,
    #[arg(long = "acp-profile", allow_hyphen_values = true)]
    profile: Option<String>,
    #[arg(long = "acp-sign-in", allow_hyphen_values = true)]
    sign_in: Option<String>,
    #[arg(long = "acp-agent", allow_hyphen_values = true)]
    agent: Option<String>,
    #[arg(long = "acp-version", allow_hyphen_values = true)]
    version: Option<String>,
    #[arg(long = "acp-agents-dir", allow_hyphen_values = true)]
    agents_dir: Option<PathBuf>,
    #[arg(long = "acp-model", allow_hyphen_values = true)]
    model: Option<String>,
    /// Counted, not a flag field: how many times `--acp-list-models` was given.
    #[arg(long = "acp-list-models", action = ArgAction::Count)]
    list_models: u8,
}

impl AcpFlags {
    /// Whether any `--acp-*` option was given.
    fn given(&self) -> bool {
        *self != Self::default()
    }

    /// The agent the options ask for: the planner unless `--agent acp`, and then the spec,
    /// checked. `--acp-*` options without `--agent acp` are a mistake, not an ignored line.
    fn agent(self, word: Option<String>) -> Result<Agent, UsageError> {
        match word.as_deref() {
            None | Some("planner") if !self.given() => Ok(Agent::Planner),
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
        spec.task = match self.list_models {
            0 => Task::default(),
            _ => Task::ListModels,
        };
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
