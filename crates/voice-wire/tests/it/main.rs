mod props;
mod wire;

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
