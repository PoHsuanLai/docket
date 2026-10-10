mod acp_agent;
mod acp_agent_exec;
mod acp_agent_grants;
mod acp_agent_label;
mod acp_agent_one_sheet;
mod acp_agent_paths;
mod acp_agent_reads;
mod acp_agent_tools;
mod activation;
mod auth;
mod behaviour;
mod call_step;
mod checkpoints;
mod companion;
mod control;
mod crossspace;
mod editor_policy;
mod gate;
mod index;
mod lifecycle;
mod live_settings;
mod machines;
mod messaging;
mod notes;
mod percall;
mod perform;
mod recall;
mod related;
mod restore;
mod restore_owner;
mod sessions;
mod shell_defaults;
mod space_owner;
mod standing;
mod standing_editor;
mod stored;
mod support;
mod tasks;
mod terminal;
mod terminal_sessions;
mod terminal_surface;
mod watching;

/// Every test file is one module of this binary: a `tests/*.rs` file would be a binary of its
/// own, and a `tests/it/` file or directory without a `mod` line would never run.
#[test]
fn every_test_file_is_reached_by_a_mod() {
    let tests = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let main = include_str!("main.rs");
    let mut stray = Vec::new();
    for entry in std::fs::read_dir(&tests).expect("tests dir").flatten() {
        let (name, path) = (entry.file_name(), entry.path());
        if path.extension().is_some_and(|e| e == "rs") {
            stray.push(format!("tests/{}: its own binary", name.to_string_lossy()));
        }
    }
    for entry in std::fs::read_dir(tests.join("it"))
        .expect("it dir")
        .flatten()
    {
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let is_module = path.is_dir() || path.extension().is_some_and(|e| e == "rs");
        let declared =
            main.contains(&format!("mod {stem};")) || main.contains(&format!("mod {stem} {{"));
        if is_module && stem != "main" && !declared {
            stray.push(format!("tests/it/{stem}: no `mod {stem};` in main.rs"));
        }
    }
    assert!(stray.is_empty(), "not reached: {stray:#?}");
}
