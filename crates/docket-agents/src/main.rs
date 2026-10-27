//! `docket-agents`: the commands that reach the network for agents.
//!
//! ```text
//! docket-agents --dir DIR installed           what is installed, without the network: one JSON
//!                                             object a line (id, versions)
//! docket-agents --dir DIR list                every agent on offer: one JSON object a line
//!                                             (id, name, description, version, installed)
//! docket-agents --dir DIR check ID            what the registry offers against what is installed
//! docket-agents --dir DIR install ID VERSION  install exactly that version
//! ```
//! `--registry-file FILE` reads the snapshot from a file instead of the registry. `install` says
//! `installing ID VERSION` on its first line before it reaches the network.

use clap::{Parser, Subcommand};
use docket_agents::fetch::{Curl, Fetch};
use docket_agents::run::System;
use docket_agents::{AgentsDir, Snapshot, Standing, Want, install, offers, standing};
use std::path::PathBuf;
use std::process::ExitCode;

const REGISTRY: &str = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";

/// The command line.
#[derive(Debug, Parser)]
#[command(
    name = "docket-agents",
    about = "Coding agents from the agent registry"
)]
struct Cli {
    /// The agents directory.
    #[arg(long, global = true, value_name = "DIR")]
    dir: PathBuf,
    /// Read the registry snapshot from this file instead of the registry.
    #[arg(long, global = true, value_name = "FILE")]
    registry_file: Option<PathBuf>,
    #[command(subcommand)]
    command: Request,
}

/// What is asked.
#[derive(Debug, Subcommand)]
enum Request {
    /// What is installed, without the network.
    Installed,
    /// Every agent on offer.
    List,
    /// What the registry offers against what is installed.
    Check {
        /// The agent's registry id.
        id: String,
    },
    /// Install exactly that version.
    Install {
        /// The agent's registry id.
        id: String,
        /// The version to install.
        version: String,
    },
}

fn snapshot(file: Option<PathBuf>) -> Result<Snapshot, String> {
    let text = match file {
        Some(path) => std::fs::read_to_string(path).map_err(|e| e.to_string())?,
        None => {
            let bytes = Curl.get(REGISTRY).map_err(|e| e.to_string())?;
            String::from_utf8(bytes).map_err(|e| e.to_string())?
        }
    };
    Snapshot::parse(&text).map_err(|e| e.to_string())
}

/// Prints each installed agent and its versions, one JSON object a line.
fn installed(dir: &AgentsDir) -> ExitCode {
    for id in dir.agents() {
        let versions: Vec<String> = dir
            .versions(&id)
            .iter()
            .map(|v| v.as_str().to_owned())
            .collect();
        let line = serde_json::json!({ "id": id.as_str(), "versions": versions });
        println!("{line}");
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let dir = AgentsDir::at(&cli.dir);
    if let Request::Installed = cli.command {
        return installed(&dir);
    }
    let snap = match snapshot(cli.registry_file) {
        Ok(snap) => snap,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::FAILURE;
        }
    };
    match &cli.command {
        Request::Installed => installed(&dir),
        Request::List => {
            for offer in offers(&dir, &snap) {
                match serde_json::to_string(&offer) {
                    Ok(line) => println!("{line}"),
                    Err(why) => {
                        eprintln!("{why}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            ExitCode::SUCCESS
        }
        Request::Check { id } => {
            match standing(&dir, &snap, id) {
                Standing::Unknown => println!("the list has no {id}"),
                Standing::Current(v) => {
                    println!("{id} {v} is installed and is the version on offer")
                }
                Standing::Offered { offered, installed } => println!(
                    "{id} {offered} is on offer; installed: {}. To take it: docket-agents install {id} {offered}",
                    if installed.is_empty() {
                        "nothing".to_owned()
                    } else {
                        installed.join(", ")
                    }
                ),
            }
            ExitCode::SUCCESS
        }
        Request::Install { id, version } => {
            println!("installing {id} {version}");
            match install(&dir, &snap, Want { id, version }, (&Curl, &System)) {
                Ok(done) => {
                    println!(
                        "installed {} {}; pin `version = \"{}\"` in agents.toml",
                        done.id, done.version, done.version
                    );
                    ExitCode::SUCCESS
                }
                Err(why) => {
                    eprintln!("{why}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}
