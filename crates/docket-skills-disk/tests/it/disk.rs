//! The disk folder lists sorted directories and reads files, and says why it could not.

use docket_skills::{Origin, Roots, SkillFault, SkillFolder, discover, load_dir};
use docket_skills_disk::DiskFolder;

fn write(root: &std::path::Path, id: &str, toml: &str, md: &str) {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join("skill.toml"), toml).expect("toml");
    std::fs::write(dir.join("SKILL.md"), md).expect("md");
}

const TOML: &str = "vocab = 1\nid = \"a\"\nowner = \"org.quire.Companion\"\nversion = \"0.1.0\"\n\
                    uses = [\"org.quire.Companion:companion.skill.load\"]\n";
const MD: &str = "---\nname: a\ndescription: on disk\n---\nBody\n";

#[test]
fn subdirs_are_sorted_files_are_left_out_and_a_missing_root_is_empty() {
    let tmp = tempfile::tempdir().expect("tmp");
    std::fs::create_dir(tmp.path().join("b")).expect("b");
    std::fs::create_dir(tmp.path().join("a")).expect("a");
    std::fs::write(tmp.path().join("file"), "x").expect("file");
    let names: Vec<_> = DiskFolder
        .subdirs(tmp.path())
        .iter()
        .map(|p| p.file_name().expect("name").to_owned())
        .collect();
    assert_eq!(names, ["a", "b"]);
    assert!(DiskFolder.subdirs(&tmp.path().join("none")).is_empty());
}

#[test]
fn a_skill_on_disk_loads_and_a_missing_file_is_unreadable() {
    let tmp = tempfile::tempdir().expect("tmp");
    write(tmp.path(), "a", TOML, MD);
    let skill = load_dir(&DiskFolder, &tmp.path().join("a"), Origin::Shipped).expect("loads");
    assert_eq!(skill.description, "on disk");
    std::fs::create_dir(tmp.path().join("empty")).expect("dir");
    let fault = load_dir(&DiskFolder, &tmp.path().join("empty"), Origin::Shipped);
    assert!(matches!(
        fault,
        Err(SkillFault::Unreadable {
            file: "skill.toml",
            ..
        })
    ));
    let found = discover(
        &Roots::shipped_only(vec![tmp.path().to_owned()]),
        &DiskFolder,
    );
    assert_eq!(found.skills.len(), 1);
    assert_eq!(found.rejected.len(), 1);
}
