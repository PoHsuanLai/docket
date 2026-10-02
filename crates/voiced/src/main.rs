//! The voiced binary: a skeleton until the PipeWire device and the serving loop exist.

use clap::Parser;
use std::process::ExitCode;

/// The voice daemon.
#[derive(Debug, Parser)]
#[command(name = "voiced", about = "The voice daemon (org.quire.Voice1)")]
struct Args {
    /// Path of voiced.toml.
    #[arg(long, default_value = "/etc/quire/voiced.toml")]
    config: std::path::PathBuf,
}

fn main() -> ExitCode {
    let _args = Args::parse();
    eprintln!("voiced: not implemented");
    ExitCode::from(2)
}
