//! What the daemons read from the scratch root: the callers tables, inferd's configuration, the
//! Spaces.

use super::ModelSource;
use std::path::Path;

/// inferd's caller table, a system file: the apps by the units that run them. inferd reads who is
/// calling from this file alone, never from `inferd.toml`.
pub const INFERD_CALLERS: &str = "[apps]\n\"org.quire.Memory\" = [\"memoryd.service\"]\n\"org.quire.Intents\" = [\"intentd.service\"]\n\"org.quire.Companion\" = [\"companiond.service\", { unit = \"docket-acp.scope\", exe = [\"/usr/bin/docket-acp\"] }]\n\"org.quire.Reader\" = [\"readerd.service\"]\n";

/// accountd's caller table, a system file (its `callers.toml`): inferd is the AI broker, the one
/// daemon that asks for the key behind a permit, and the launcher of external coding agents may
/// call the `Launcher` interface.
pub const ACCOUNTD_CALLERS: &str = r#"[[caller]]
app = "org.quire.Inference"
unit = "inferd.service"
role = "ai_broker"
name = "Intelligence"

[[caller]]
app = "org.quire.DocketAcp"
unit = "docket-acp.service"
role = "agent_launcher"
name = "Agents"
"#;

/// The shipped callers files of the daemons that read their own, by file name.
const SYSTEM_CALLERS: [(&str, &str); 7] = [
    ("memory-callers.toml", MEMORY_CALLERS),
    ("accountd-callers.toml", ACCOUNTD_CALLERS),
    ("inferd-callers.toml", INFERD_CALLERS),
    (
        "intentd-callers.toml",
        include_str!("../../../../dist/intentd-callers.toml"),
    ),
    (
        "companiond-callers.toml",
        include_str!("../../../../dist/companiond-callers.toml"),
    ),
    (
        "readerd-callers.toml",
        include_str!("../../../../dist/readerd-callers.toml"),
    ),
    (
        "acp-callers.toml",
        include_str!("../../../../dist/acp-callers.toml"),
    ),
];

/// Writes the shipped callers files into `<root>/system/`: the harness is its own system, and only
/// the system's file can give a fixed role (the shell's `sheet_host`), so these are the daemons'
/// system files, not the person's (`<root>/config/quire`).
pub fn write_system_callers(root: &Path) -> std::io::Result<()> {
    let dir = root.join("system");
    std::fs::create_dir_all(&dir)?;
    for (file, text) in SYSTEM_CALLERS {
        std::fs::write(dir.join(file), text)?;
    }
    Ok(())
}

/// inferd's configuration. A cassette: the replay engine `scripted` playing the scratch cassette
/// (recording its requests when `record` names a file). A live source: the owner's body as it is.
/// A cassette also names the replay engine's model for every tier of the text slot: with no
/// default planner, a tier with no row is refused as no model chosen. The callers are not here:
/// they are inferd's system file (`inferd-callers.toml`).
pub fn inferd_toml(root: &Path, model: &ModelSource, record: Option<&Path>) -> String {
    match model {
        ModelSource::Scripted(_) => {
            let chosen = SCRIPTED_TEXT;
            let record = record
                .map(|p| format!("record = {:?}\n", p.display().to_string()))
                .unwrap_or_default();
            format!(
                "[engines.scripted]\nreplay = {:?}\n{record}\n{chosen}",
                root.join("cassette.jsonl").display().to_string()
            )
        }
        ModelSource::Live(body) => format!("{body}\n"),
    }
}

/// The person's choice for the replay engine's world: its model at every tier of the text slot.
const SCRIPTED_TEXT: &str = "[ai.model.text]\nfast = \"local/scripted\"\nbalanced = \"local/scripted\"\nbest = \"local/scripted\"\n";

/// memoryd's callers file: the units that may call it, with the roles the router and the shell
/// are told apart by. The shell is `sill.service` only; the run places the test process
/// in that unit, as its main process.
pub const MEMORY_CALLERS: &str = r#"[[caller]]
app = "org.quire.Intents"
unit = "intentd.service"
role = "agent"

[[caller]]
app = "org.quire.Cua"
unit = "cuad.service"
role = "cua"

[[caller]]
app = "org.quire.Shell"
unit = "sill.service"
role = "sheet_host"

[[caller]]
app = "org.quire.Companion"
unit = "companiond.service"
role = "agent"

[[caller]]
app = "org.quire.Acp"
unit = "docket-acp.scope"
role = "agent"
exe = ["/usr/bin/docket-acp"]

[[caller]]
app = "org.quire.Reader"
unit = "readerd.service"
role = "agent"
"#;

/// Writes `text` to `path`, making its directories.
pub fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("dirs");
    std::fs::write(path, text).expect("file");
}

/// `spaces.toml`: the Space `work`, and `desktop` where intentd places the records that name no
/// Space (a confirmation, an undo), both sealed with fixed replicas.
pub const SPACES: &str = r#"[[spaces]]
id = "work"
created = 0
replica = "00000000000000000000000000000001"
vault = "sealed"
format = 1

[[spaces]]
id = "desktop"
created = 0
replica = "00000000000000000000000000000002"
vault = "sealed"
format = 1
"#;
