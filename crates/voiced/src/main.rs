//! The voiced binary: reads `voiced.toml`, then serves `org.quire.Voice1` on the session bus over
//! PipeWire until the bus closes.

use clap::Parser;
use std::process::ExitCode;
use voiced::{FileUse, PipeWireDevice, VoicedConfig};

/// The voice daemon.
#[derive(Debug, Parser)]
#[command(name = "voiced", about = "The voice daemon (org.quire.Voice1)")]
struct Args {
    /// Path of voiced.toml.
    #[arg(long, default_value = "/etc/quire/voiced.toml")]
    config: std::path::PathBuf,
}

fn load(path: &std::path::Path) -> Result<VoicedConfig, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    VoicedConfig::parse(&text).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    let args = Args::parse();
    let config = match load(&args.config) {
        Ok(config) => config,
        Err(why) => {
            eprintln!("voiced: {why}");
            return ExitCode::FAILURE;
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(why) => {
            eprintln!("voiced: no runtime: {why}");
            return ExitCode::FAILURE;
        }
    };
    let usage = FileUse::sill_default(|name| std::env::var(name).ok());
    match runtime.block_on(voiced::serve(config, PipeWireDevice, usage)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("voiced: {why}");
            ExitCode::FAILURE
        }
    }
}
