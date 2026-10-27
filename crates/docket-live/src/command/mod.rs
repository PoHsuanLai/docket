//! The two commands of `docket-live`: `corpus` plays the eval corpora through the router with the
//! real policy writer and reviewer cascade over a model; `smoke` plays the acceptance flows with
//! every daemon against a model. The binary only parses its command line and hands the command here.

mod corpus;
mod smoke;

use crate::cli::{Agent, Command};
use crate::{Engine, Reach};
use docket_accept::world::Binaries;
use std::process::ExitCode;

/// Runs `command` against the daemons in `binaries`. The error is what the program says before it
/// exits with code 2.
pub async fn run(command: Command, binaries: &Binaries) -> Result<ExitCode, String> {
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
