//! The policy files the daemon reads: the preset of the first data directory that has one, the
//! person's and the assistant's, and the administrator's; and a rebuild put in force on a router.

use docket_core::AgentConfig;
use docket_fake::fake_router;
use docket_router::{Router, Seams};
use intentd::{PolicyDirs, load_policies, read_sources, reload_policies};
use policy_point::{Layer, LayerError, STANDARD_PRESET};
use std::path::Path;

const NO_READS: &str = r#"
    @id("no-reads")
    forbid (principal, action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"], resource)
    when { resource.effect == "read" };
"#;

fn put(dir: &Path, name: &str, text: &str) {
    std::fs::create_dir_all(dir).expect("dir");
    std::fs::write(dir.join(name), text).expect("write");
}

struct Scratch {
    mine: tempfile::TempDir,
    system: tempfile::TempDir,
    config: tempfile::TempDir,
    admin: tempfile::TempDir,
}

impl Scratch {
    fn new() -> Self {
        Self {
            mine: tempfile::tempdir().expect("scratch"),
            system: tempfile::tempdir().expect("scratch"),
            config: tempfile::tempdir().expect("scratch"),
            admin: tempfile::tempdir().expect("scratch"),
        }
    }

    fn dirs(&self) -> PolicyDirs {
        PolicyDirs {
            data_dirs: vec![
                self.mine.path().to_path_buf(),
                self.system.path().to_path_buf(),
            ],
            person: self.config.path().join("quire/permissions"),
            admin: self.admin.path().to_path_buf(),
        }
    }
}

fn has_rule<S: Seams>(router: &Router<S>) -> bool {
    router
        .pdp()
        .policy_ids()
        .iter()
        .any(|id| id.0 == "no-reads")
}

fn layers(dirs: &PolicyDirs) -> Vec<Layer> {
    read_sources(dirs).iter().map(|s| s.layer).collect()
}

#[test]
fn every_layer_is_read_in_name_order_and_the_first_preset_wins() {
    let scratch = Scratch::new();
    let dirs = scratch.dirs();
    assert_eq!(
        layers(&dirs),
        [Layer::Core],
        "nothing installed: the built-in preset stands"
    );
    let preset = |root: &Path| root.join("quire/permissions/standard");
    put(
        &preset(scratch.system.path()),
        "grid.cedar",
        STANDARD_PRESET,
    );
    put(
        &preset(scratch.mine.path()),
        "notes.txt",
        "not a policy file",
    );
    put(&preset(scratch.mine.path()), "grid.cedar", STANDARD_PRESET);
    put(&dirs.person, "b.cedar", NO_READS);
    put(&dirs.person, "a.cedar", NO_READS);
    put(&dirs.assistant(), "x.cedar", NO_READS);
    put(&dirs.admin, "floor.cedar", NO_READS);
    let sources = read_sources(&dirs);
    let files: Vec<String> = sources.iter().map(|s| s.origin.to_string()).collect();
    assert!(
        files[1].starts_with(&scratch.mine.path().display().to_string()),
        "{files:?}"
    );
    assert!(
        files[2].ends_with("a.cedar") && files[3].ends_with("b.cedar"),
        "{files:?}"
    );
    assert_eq!(
        sources.iter().map(|s| s.layer).collect::<Vec<_>>(),
        [
            Layer::Core,
            Layer::Preset,
            Layer::Person,
            Layer::Person,
            Layer::Assistant,
            Layer::Admin
        ]
    );
}

#[test]
fn a_bad_person_file_is_skipped_and_a_bad_administrator_file_is_an_error() {
    let scratch = Scratch::new();
    let dirs = scratch.dirs();
    put(&dirs.person, "bad.cedar", "forbid (");
    let loaded = load_policies(&dirs).expect("loads");
    assert_eq!(loaded.skipped.len(), 1);
    put(&dirs.admin, "bad.cedar", "forbid (");
    assert!(matches!(load_policies(&dirs), Err(LayerError::Admin(_))));
}

#[test]
fn a_rebuild_is_put_in_force_on_the_router_and_a_failed_one_leaves_it_alone() {
    let scratch = Scratch::new();
    let dirs = scratch.dirs();
    let router = fake_router(AgentConfig::default()).expect("router");
    assert!(!has_rule(&router));
    put(&dirs.person, "mine.cedar", NO_READS);
    reload_policies(&router, &dirs);
    assert!(has_rule(&router));
    put(&dirs.admin, "bad.cedar", "forbid (");
    std::fs::remove_file(dirs.person.join("mine.cedar")).expect("remove");
    reload_policies(&router, &dirs);
    assert!(
        has_rule(&router),
        "the set in force stays when the administrator's file does not load"
    );
}
