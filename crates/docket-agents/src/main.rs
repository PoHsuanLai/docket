//! `docket-agents`: the commands that reach the network for agents.
//!
//! ```text
//! docket-agents --dir DIR check ID            what the registry offers against what is installed
//! docket-agents --dir DIR install ID VERSION  install exactly that version
//! ```
//! `--registry-file FILE` reads the snapshot from a file instead of the registry.

use docket_agents::fetch::{Curl, Fetch};
use docket_agents::run::System;
use docket_agents::{AgentsDir, Snapshot, Standing, Want, install, standing};
use std::path::PathBuf;
use std::process::ExitCode;

const REGISTRY: &str = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";

fn usage() -> ExitCode {
    eprintln!(
        "usage: docket-agents --dir DIR [--registry-file FILE] (check ID | install ID VERSION)"
    );
    ExitCode::from(2)
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

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (mut dir, mut file, mut rest) = (None, None, Vec::new());
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dir" => dir = args.next().map(PathBuf::from),
            "--registry-file" => file = args.next().map(PathBuf::from),
            _ => rest.push(arg),
        }
    }
    let Some(dir) = dir.map(|d| AgentsDir::at(&d)) else {
        return usage();
    };
    let snap = match snapshot(file) {
        Ok(snap) => snap,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::FAILURE;
        }
    };
    match rest
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["check", id] => {
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
        ["install", id, version] => {
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
        _ => usage(),
    }
}
