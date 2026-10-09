mod acp_gate;
mod audit;
mod binary;
mod bus_members;
mod files;
mod gate_watch;
mod hosted;
mod identity;
mod infer;
mod inferd_link;
mod link;
mod logout;
mod parts;
mod percall;
mod perform_watch;
mod reader_bus;
mod reader_daemon;
mod settings_live;
mod sheet;
mod signals;
mod spaces;
mod support;
mod writer;
mod writer_ceiling;
mod writer_hostile;

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
