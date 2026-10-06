//! The command line of `docket-live`, parsed into a request. Pure: it reads no file and no
//! environment, so a table of command lines tests it.

use crate::live::engine::{Engine, EngineError};
use std::path::PathBuf;

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
    /// Fail when a corpus's false-negative rate (the Wilson upper end) exceeds this, in
    /// thousandths.
    pub fnr_max_permille: Option<u32>,
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
    /// Only these flows; every one when empty.
    pub flows: Vec<String>,
    /// Seconds to wait for each change of an answer.
    pub patience_s: u64,
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
    let (mut timeout, mut accountd, mut fnr, mut patience) = (None, None, None, 600_u64);
    while let Some(flag) = words.0.next() {
        match flag.as_str() {
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
            "--accountd" => accountd = Some(PathBuf::from(words.value(flag)?)),
            "--fnr-max-permille" => fnr = Some(number(flag, words.value(flag)?)?),
            "--patience-s" => patience = number(flag, words.value(flag)?)?,
            other => return Err(UsageError::Unknown(other.to_owned())),
        }
    }
    let engine = engine(engine_text)?;
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
            fnr_max_permille: fnr,
        })),
        "smoke" => Ok(Command::Smoke(SmokeArgs {
            engine,
            inferd_config: config,
            out,
            flows,
            patience_s: patience,
        })),
        _ => Err(UsageError::Subcommand),
    }
}
