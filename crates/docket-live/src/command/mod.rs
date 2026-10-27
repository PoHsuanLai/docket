//! The two commands of `docket-live`: `corpus` plays the eval corpora through the router with the
//! real policy writer and reviewer cascade over a model; `smoke` plays the acceptance flows with
//! every daemon against a model. The binary only parses its command line and hands the command here.

mod corpus;
mod smoke;

use crate::acp::PlayFault;
use crate::cli::{Agent, Command};
use crate::regress::RegressError;
use crate::report_file::ReportFault;
use crate::{Engine, EngineError, Reach};
use docket_accept::keys_home::HomeFault;
use docket_accept::world::Binaries;
use eval_core::{AttackError, SummaryError};
use std::process::ExitCode;

/// Why a command did not run. The words are what the program prints after `docket-live: `.
#[derive(Debug, thiserror::Error)]
pub enum CommandFault {
    /// The corpus cases could not be read.
    #[error(transparent)]
    Cases(#[from] docket_eval::CorpusError),
    /// The planner cases could not be read.
    #[error(transparent)]
    PlannerCases(#[from] docket_eval::PlannerCaseError),
    /// The attacks could not be read or scored.
    #[error(transparent)]
    Attacks(#[from] AttackError),
    /// The attack summary could not be made or written.
    #[error(transparent)]
    Summary(#[from] SummaryError),
    /// The regression pairs could not be read.
    #[error(transparent)]
    Regress(#[from] RegressError),
    /// A run could not be made.
    #[error(transparent)]
    Run(#[from] crate::CorpusError),
    /// The model source is wrong.
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// The key home is not usable.
    #[error(transparent)]
    Home(#[from] HomeFault),
    /// The report could not be written.
    #[error(transparent)]
    Report(#[from] ReportFault),
    /// The agent could not be hosted.
    #[error(transparent)]
    Play(#[from] PlayFault),
    /// A trace directory could not be made or written.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Nothing the command line names is a case.
    #[error("no case matches --corpus/--case")]
    NoCase,
    /// `--flow` names no flow or planner case.
    #[error("no flow or planner case named {0:?}")]
    NoFlow(String),
    /// `--flow` names no flow an agent can play.
    #[error("no flow named {0:?} (a planner case has no agent counterpart)")]
    NoAgentFlow(String),
    /// `--attack-summary` with no attack played.
    #[error("--attack-summary: this run played no attacks (--corpus attacks)")]
    NoAttackSummary,
    /// `--planner live` with another corpus or `--regress`.
    #[error(
        "--planner live plays the attack suite only: name --corpus attacks (or --case <attack id>), and no other corpus or --regress"
    )]
    LivePlannerOnlyAttacks,
    /// `--planner live` with no attack named.
    #[error("--planner live plays the attack suite: name --corpus attacks (or --case <attack id>)")]
    NoAttacks,
    /// The regression directory held nothing to run.
    #[error("nothing to run")]
    NothingToRun,
}

/// Runs `command` against the daemons in `binaries`. The error is what the program says before it
/// exits with code 2.
pub async fn run(command: Command, binaries: &Binaries) -> Result<ExitCode, CommandFault> {
    match command {
        Command::Corpus(args) => corpus::corpus(args, binaries).await,
        Command::Smoke(args) => smoke::smoke(args, binaries).await,
    }
}

pub(super) fn say_reach(engine: Engine) {
    if engine.reach() == Reach::Network {
        eprintln!(
            "docket-live: --engine cloud: prompts WILL be sent over the network to the hosted model your inferd config names"
        );
    } else {
        eprintln!("docket-live: --engine {}: no network", engine.slug());
    }
}

/// What an external agent reaches, said before anything starts: its own network, whatever the
/// engine is, and the login it is given.
pub(super) fn say_agent(agent: &Agent) {
    if let Agent::Acp(spec) = agent {
        eprintln!(
            "docket-live: --agent acp: {} runs in a sandbox with network {}{}",
            spec.program,
            crate::acp::spec::network_word(spec.network),
            if spec.network == bulkhead::NetworkMode::Host {
                " (its prompts go to its own provider)"
            } else {
                ""
            }
        );
        if spec.credentials.is_some() {
            eprintln!(
                "docket-live: the login you named is copied into the scratch HOME for this run and removed when it ends"
            );
        }
    }
}
