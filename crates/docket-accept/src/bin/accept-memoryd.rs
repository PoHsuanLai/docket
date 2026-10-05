//! memoryd as its own process, as almanac's `main.rs` builds it, with two differences: the keys
//! are in memory (the private bus has no Secret Service, and the real daemon's `Oo7Keys` needs
//! one), and no Landlock sandbox is applied. Everything else is memoryd's library: the SQLCipher
//! logs, the sealed files, the bus handler, and callers named by their executable through
//! `memory-callers.toml`.

#![recursion_limit = "512"]

use almanac_core::{Dirs, RuleSet};
use almanac_dbus::serve_on;
use almanac_seal::MemoryKeys;
use almanac_service::{MemoryService, rules_from_toml, spaces_from_toml};
use memoryd::{
    CallerTable, Daemon, InferdConsolidator, InferdEmbedder, ProcPeers, SystemBackend,
    default_card, dirs_from_env, inferd_link,
};
use std::process::ExitCode;
use std::sync::Arc;

fn callers_toml(dirs: &Dirs) -> std::path::PathBuf {
    dirs.memory_toml().with_file_name("memory-callers.toml")
}

async fn run(dirs: Dirs) -> Result<(), String> {
    let connection = zbus::connection::Builder::session()
        .map_err(|e| e.to_string())?
        .build()
        .await
        .map_err(|e| e.to_string())?;
    let inferd = inferd_link(&connection);
    let backend = SystemBackend::with(
        dirs.clone(),
        MemoryKeys::default(),
        InferdEmbedder::new(inferd.clone(), default_card()),
        InferdConsolidator::new(inferd),
    );
    let rules = std::fs::read_to_string(dirs.memory_toml())
        .ok()
        .and_then(|t| rules_from_toml(&t).ok())
        .unwrap_or_else(RuleSet::standard);
    let service = MemoryService::new(backend, rules);
    if let Ok(text) = std::fs::read_to_string(dirs.spaces_toml()) {
        let file = spaces_from_toml(&text).map_err(|e| format!("spaces.toml: {e}"))?;
        file.spaces
            .into_iter()
            .for_each(|meta| service.register(meta));
    }
    let table = std::fs::read_to_string(callers_toml(&dirs))
        .map_err(|e| e.to_string())
        .and_then(|t| CallerTable::from_toml(&t))?;
    let peers = ProcPeers::new(connection.clone(), table);
    let daemon = Arc::new(Daemon::new(service, peers, dirs));
    serve_on(&connection, daemon.clone())
        .await
        .map_err(|e| e.to_string())?;
    daemon.attach(connection);
    std::future::pending::<()>().await;
    Ok(())
}

fn main() -> ExitCode {
    let started = dirs_from_env().map_err(|e| e.to_string()).and_then(|dirs| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?
            .block_on(run(dirs))
    });
    match started {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("memoryd: {why}");
            ExitCode::from(1)
        }
    }
}
