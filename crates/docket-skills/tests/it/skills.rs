//! Tables for the parser and the validator, discovery, reachability, preselection and the load
//! cap.

use docket_core::{AgentReach, SkillId, ValidManifest};
use docket_skills::{
    Always, BODY_BUDGET_BYTES, Found, LOAD_MAX, Library, LoadRefusal, Loaded, Origin, Reach, Roots,
    Situation, SkillFault, discover, load_dir, parse, parse_doc, parse_facts, preselect, reach,
    uses_first,
};
use porter_core::AppName;
use prov::EntityKind;
use std::collections::BTreeSet;
use std::path::PathBuf;

const COMPANION: &str = include_str!("../../../../manifests/org.quire.Companion.toml");

fn companion() -> ValidManifest {
    toml::from_str(COMPANION).expect("the shipped manifest validates")
}

fn toml_for(id: &str, extra: &str) -> String {
    format!(
        "vocab = 1\nid = \"{id}\"\nowner = \"org.quire.Companion\"\nversion = \"0.1.0\"\n\
         uses = [\"org.quire.Companion:companion.skill.load\"]\n{extra}"
    )
}

fn md_for(name: &str, description: &str, body: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\n{body}\n")
}

fn skill(id: &str, extra: &str) -> docket_skills::Skill {
    parse(
        id,
        &toml_for(id, extra),
        &md_for(id, "d", "body"),
        Origin::Shipped,
    )
    .expect("skill")
}

#[test]
fn skill_toml_table() {
    let ok = toml_for("a-b", "");
    let cases: Vec<(String, Result<(), &str>)> = vec![
        (ok.clone(), Ok(())),
        (ok.replace("vocab = 1", "vocab = 2"), Err("vocab")),
        (ok.replace("id = \"a-b\"", "id = \"A_B\""), Err("bad id")),
        (ok.replace("0.1.0\"", "0.1.0\"\nsurprise = 1"), Err("toml")),
        (
            ok.replace("companion.skill.load", "nocolon"),
            Err("bad use"),
        ),
        (
            ok.replace("org.quire.Companion:", "Bad App:"),
            Err("bad use"),
        ),
        (
            format!("{ok}[when]\nkinds = [\"Not A Kind\"]\n"),
            Err("bad kind"),
        ),
        (format!("{ok}[when]\nalways = \"maybe\"\n"), Err("toml")),
        (format!("{ok}[when]\nalways = true\n"), Err("toml")),
        (format!("{ok}[when]\nalways = \"yes\"\n"), Ok(())),
        (ok.replace("owner", "own"), Err("toml")),
    ];
    for (text, want) in cases {
        let got = parse_facts(&text);
        match (&got, want) {
            (Ok(_), Ok(())) => {}
            (Err(SkillFault::Vocab(_)), Err("vocab")) => {}
            (Err(SkillFault::BadId(_)), Err("bad id")) => {}
            (Err(SkillFault::Toml(_)), Err("toml")) => {}
            (Err(SkillFault::BadUse(_)), Err("bad use")) => {}
            (Err(SkillFault::BadKind(_)), Err("bad kind")) => {}
            _ => panic!("{text}\n=> {got:?}, wanted {want:?}"),
        }
    }
}

#[test]
fn skill_md_table() {
    type Expected = Result<(&'static str, &'static str, &'static str), SkillFault>;
    let cases: Vec<(&str, Expected)> = vec![
        (
            "---\nname: x\ndescription: Does x.\n---\n# Title\n\nText.\n",
            Ok(("x", "Does x.", "# Title\n\nText.")),
        ),
        (
            "---\nname: \"x\"\ndescription: 'Does x.'\nlicense: MIT\nmetadata:\n  a: b\n---\nText\n",
            Ok(("x", "Does x.", "Text")),
        ),
        (
            "---\r\nname: x\r\ndescription: d\r\n---\r\nText\r\n",
            Ok(("x", "d", "Text")),
        ),
        ("# no front matter\n", Err(SkillFault::NoFrontMatter)),
        (
            "---\nname: x\ndescription: d\n",
            Err(SkillFault::NoFrontMatter),
        ),
        (
            "---\ndescription: d\n---\nb",
            Err(SkillFault::MissingKey("name")),
        ),
        (
            "---\nname: x\n---\nb",
            Err(SkillFault::MissingKey("description")),
        ),
        (
            "---\nname: x\ndescription:\n---\nb",
            Err(SkillFault::MissingKey("description")),
        ),
    ];
    for (text, want) in cases {
        let got = parse_doc(text).map(|d| (d.name, d.description, d.body));
        let want = want.map(|(a, b, c)| (a.to_owned(), b.to_owned(), c.to_owned()));
        assert_eq!(got, want, "{text:?}");
    }
}

#[test]
fn rules_of_the_format_table() {
    let long_description = "d".repeat(161);
    let exact_description = "d".repeat(160);
    let exact_body = "b".repeat(8192);
    let big_body = "b".repeat(8193);
    let cases: Vec<(&str, String, String, Result<(), SkillFault>)> = vec![
        ("a", toml_for("a", ""), md_for("a", "d", "body"), Ok(())),
        (
            "a",
            toml_for("a", ""),
            md_for("a", &exact_description, &exact_body),
            Ok(()),
        ),
        (
            "a",
            toml_for("a", ""),
            md_for("a", &long_description, "body"),
            Err(SkillFault::DescriptionTooLong(161)),
        ),
        (
            "a",
            toml_for("a", ""),
            md_for("a", "d", &big_body),
            Err(SkillFault::BodyTooLarge(8193)),
        ),
        (
            "a",
            toml_for("a", ""),
            md_for("a", "d", ""),
            Err(SkillFault::EmptyBody),
        ),
        (
            "a",
            toml_for("b", ""),
            md_for("a", "d", "x"),
            Err(SkillFault::IdMismatch {
                dir: "a".into(),
                toml: "b".into(),
                md: "a".into(),
            }),
        ),
        (
            "a",
            toml_for("a", ""),
            md_for("c", "d", "x"),
            Err(SkillFault::IdMismatch {
                dir: "a".into(),
                toml: "a".into(),
                md: "c".into(),
            }),
        ),
        (
            "dir",
            toml_for("a", ""),
            md_for("a", "d", "x"),
            Err(SkillFault::IdMismatch {
                dir: "dir".into(),
                toml: "a".into(),
                md: "a".into(),
            }),
        ),
    ];
    for (dir, toml, md, want) in cases {
        let got = parse(dir, &toml, &md, Origin::Shipped).map(|_| ());
        assert_eq!(got, want, "{dir} {md:.60}");
    }
}

fn write(root: &std::path::Path, id: &str, toml: &str, md: &str) {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join("skill.toml"), toml).expect("toml");
    std::fs::write(dir.join("SKILL.md"), md).expect("md");
}

#[test]
fn an_own_skill_overrides_a_shipped_one_and_a_bad_directory_is_reported() {
    let data = tempfile::tempdir().expect("tmp");
    let home = data.path().join("home");
    let sys = data.path().join("sys");
    let sys2 = data.path().join("sys2");
    let skills = |d: &PathBuf| d.join("quire").join("skills");
    write(
        &skills(&sys),
        "a",
        &toml_for("a", ""),
        &md_for("a", "shipped", "S"),
    );
    write(
        &skills(&sys),
        "b",
        &toml_for("b", ""),
        &md_for("b", "shipped b", "S"),
    );
    write(
        &skills(&sys2),
        "b",
        &toml_for("b", ""),
        &md_for("b", "second dir", "S"),
    );
    write(
        &skills(&home),
        "a",
        &toml_for("a", ""),
        &md_for("a", "mine", "M"),
    );
    write(&skills(&home), "broken", "not toml [", "x");
    let roots = Roots::in_data_dirs(Some(&home), &[sys.clone(), sys2]);
    let Found { skills, rejected } = discover(&roots);
    let described: Vec<(String, String, Origin)> = skills
        .iter()
        .map(|s| (s.id.to_string(), s.description.clone(), s.origin))
        .collect();
    assert_eq!(
        described,
        vec![
            ("a".into(), "mine".into(), Origin::Own),
            ("b".into(), "shipped b".into(), Origin::Shipped),
        ]
    );
    assert_eq!(rejected.len(), 1);
    assert!(rejected[0].dir.ends_with("broken"));
    // The label follows the origin: the person's own is the person's, shipped is the owner's.
    let own = skills[0].text();
    assert_eq!(own.label.sources, BTreeSet::from([prov::Source::User]));
    let shipped = skills[1].text();
    assert_eq!(
        shipped.label.sources,
        BTreeSet::from([prov::Source::App(
            AppName::parse("org.quire.Companion").expect("app")
        )])
    );
    assert_eq!(shipped.label.integrity, prov::Integrity::Trusted);
    assert!(load_dir(&skills_path(&sys).join("a"), Origin::Shipped).is_ok());
}

fn skills_path(root: &std::path::Path) -> PathBuf {
    root.join("quire").join("skills")
}

#[test]
fn roots_read_the_xdg_variables() {
    let env = |key: &str| match key {
        "XDG_DATA_HOME" => Some("/h".to_owned()),
        "XDG_DATA_DIRS" => Some("/a:/b".to_owned()),
        _ => None,
    };
    let roots = Roots::from_env(&env);
    assert_eq!(roots.own, Some(PathBuf::from("/h/quire/skills")));
    assert_eq!(
        roots.shipped,
        vec![
            PathBuf::from("/a/quire/skills"),
            PathBuf::from("/b/quire/skills")
        ]
    );
    let none = Roots::from_env(&|_| None);
    assert_eq!(none.own, None);
    assert_eq!(
        none.shipped,
        vec![
            PathBuf::from("/usr/local/share/quire/skills"),
            PathBuf::from("/usr/share/quire/skills")
        ]
    );
}

#[test]
fn a_skill_with_a_missing_action_is_hidden_and_reported() {
    let ok = skill("ok", "");
    let mut missing = skill("missing", "");
    missing.uses[0].name = prov::ActionName::parse("companion.no.such").expect("name");
    let mut absent_app = skill("absent", "");
    absent_app.uses[0].app = AppName::parse("org.example.Absent").expect("app");
    let manifests = [companion()];
    let library = Library::check(vec![ok, missing, absent_app], &manifests);
    let kept: Vec<&str> = library.all().iter().map(|s| s.id.as_str()).collect();
    assert_eq!(kept, ["ok"]);
    let hidden: Vec<&str> = library.hidden.iter().map(|h| h.id.as_str()).collect();
    assert_eq!(hidden, ["missing", "absent"]);
    assert_eq!(library.cards(&manifests).len(), 1);
}

#[test]
fn a_skill_whose_action_is_hidden_from_the_companion_is_kept_but_not_offered() {
    let mut manifest = companion().manifest().clone();
    for action in &mut manifest.actions {
        if action.name.as_str() == "companion.skill.load" {
            action.reach = AgentReach::Hidden;
        }
    }
    let manifests = [ValidManifest::try_from(manifest).expect("valid")];
    let s = skill("s", "");
    assert!(matches!(reach(&s, &manifests), Reach::Hidden(_)));
    let library = Library::check(vec![s], &manifests);
    assert_eq!(library.all().len(), 1);
    assert!(library.offered(&manifests).is_empty());
}

fn kind(text: &str) -> EntityKind {
    EntityKind::parse(text).expect("kind")
}

#[test]
fn preselection_follows_when_and_stops_at_two() {
    let app = |s: &str| AppName::parse(s).expect("app");
    let always = skill("basics", "[when]\nalways = \"yes\"\n");
    let by_app = skill("by-app", "[when]\nfocused_app = [\"org.quire.Mail\"]\n");
    let by_kind = skill("by-kind", "[when]\nkinds = [\"mail.thread\"]\n");
    let other = skill("other", "[when]\nfocused_app = [\"org.quire.Files\"]\n");
    let plain = skill("plain", "");
    let offered = [&always, &by_app, &by_kind, &other, &plain];
    let at = |focused: Option<&str>, kinds: &[&str]| Situation {
        focused: focused.map(app),
        kinds: kinds.iter().map(|k| kind(k)).collect(),
    };
    let ids = |at: Situation| -> Vec<String> {
        preselect(&offered, &at, BODY_BUDGET_BYTES)
            .into_iter()
            .map(|s| s.id.to_string())
            .collect()
    };
    // Always first, then by id, at most two.
    assert_eq!(ids(at(None, &[])), ["basics"]);
    assert_eq!(ids(at(Some("org.quire.Mail"), &[])), ["basics", "by-app"]);
    assert_eq!(ids(at(Some("org.quire.Files"), &[])), ["basics", "other"]);
    assert_eq!(
        ids(at(Some("org.quire.Mail"), &["mail.thread"])),
        ["basics", "by-app"]
    );
    assert_eq!(
        ids(at(Some("org.quire.Shell"), &["mail.thread"])),
        ["basics", "by-kind"]
    );
    let only = [&by_app, &by_kind, &plain];
    let got: Vec<String> = preselect(
        &only,
        &at(Some("org.quire.Mail"), &["mail.thread"]),
        BODY_BUDGET_BYTES,
    )
    .into_iter()
    .map(|s| s.id.to_string())
    .collect();
    assert_eq!(got, ["by-app", "by-kind"]);
    assert_eq!(always.when.always, Always::Yes);
}

#[test]
fn a_task_loads_at_most_three_and_a_repeat_is_free() {
    let ids: Vec<SkillId> = ["a", "b", "c", "d"]
        .iter()
        .map(|i| SkillId::parse(i).expect("id"))
        .collect();
    let mut loaded = Loaded::default();
    for id in &ids[..3] {
        assert_eq!(loaded.admit(id, 10), Ok(()));
    }
    assert_eq!(loaded.admit(&ids[3], 10), Err(LoadRefusal::OverCap));
    assert_eq!(
        loaded.admit(&ids[0], 10),
        Ok(()),
        "a skill already loaded costs nothing"
    );
    assert_eq!(loaded.ids().len(), LOAD_MAX);
}

#[test]
fn loaded_skills_actions_go_first_and_nothing_else_changes() {
    let s = skill("s", "");
    let items = ["x", "org.quire.Companion:companion.skill.load", "y"];
    let action = |i: &&str| -> docket_core::ActionRef {
        match i.split_once(':') {
            Some((app, name)) => docket_core::ActionRef {
                app: AppName::parse(app).expect("app"),
                name: prov::ActionName::parse(name).expect("name"),
            },
            None => docket_core::ActionRef {
                app: AppName::parse("org.example.X").expect("app"),
                name: prov::ActionName::parse("x.y").expect("name"),
            },
        }
    };
    let cards: Vec<(docket_core::ActionRef, &str)> =
        items.iter().map(|i| (action(i), *i)).collect();
    let out = uses_first(cards.clone(), |c| &c.0, &[&s]);
    let names: Vec<&str> = out.iter().map(|c| c.1).collect();
    assert_eq!(
        names,
        ["org.quire.Companion:companion.skill.load", "x", "y"]
    );
    assert_eq!(uses_first(cards.clone(), |c| &c.0, &[]), cards);
}

/// Skill text has one way in: directories handed to `discover`. This pins the shape so a second
/// way (a string of text, a URL, a handle) cannot be added without breaking this test.
#[test]
fn skill_text_enters_only_through_directory_paths() {
    let entry: fn(&Roots) -> Found = discover;
    let roots = Roots {
        own: Some(PathBuf::from("/nonexistent/own")),
        shipped: vec![PathBuf::from("/nonexistent/shipped")],
    };
    let found = entry(&roots);
    assert!(found.skills.is_empty() && found.rejected.is_empty());
    // Every field of `Roots` is a path; destructuring without `..` fails to compile otherwise.
    let Roots { own, shipped } = roots;
    let _: (Option<PathBuf>, Vec<PathBuf>) = (own, shipped);
}

fn sized(id: &str, when: &str, bytes: usize) -> docket_skills::Skill {
    parse(
        id,
        &toml_for(id, when),
        &md_for(id, "d", &"b".repeat(bytes)),
        Origin::Shipped,
    )
    .expect("skill")
}

#[test]
fn preselection_stops_when_the_next_body_would_not_fit() {
    let app = AppName::parse("org.quire.Shell").expect("app");
    let at = Situation {
        focused: Some(app),
        kinds: BTreeSet::new(),
    };
    let first = sized("a-first", "[when]\nalways = \"yes\"\n", 5000);
    let second = sized(
        "b-second",
        "[when]\nfocused_app = [\"org.quire.Shell\"]\n",
        8000,
    );
    let third = sized(
        "c-third",
        "[when]\nfocused_app = [\"org.quire.Shell\"]\n",
        100,
    );
    let offered = [&first, &second, &third];
    let ids = |room: usize| -> Vec<String> {
        preselect(&offered, &at, room)
            .into_iter()
            .map(|s| s.id.to_string())
            .collect()
    };
    // 5000 + 8000 is over the budget: the second is not shown, and the small third is not
    // taken in its place (it would be the third, past the cap of two, and order is kept).
    assert_eq!(ids(BODY_BUDGET_BYTES), ["a-first"]);
    assert_eq!(ids(13_000), ["a-first", "b-second"]);
    assert_eq!(ids(5000), ["a-first"]);
    assert_eq!(ids(4999), Vec::<String>::new());
    assert_eq!(ids(0), Vec::<String>::new());
}

#[test]
fn loads_stop_at_the_byte_budget_and_a_reload_is_free() {
    let id = |s: &str| SkillId::parse(s).expect("id");
    let mut loaded = Loaded::default();
    assert_eq!(loaded.admit(&id("a"), 8000), Ok(()));
    assert_eq!(loaded.admit(&id("b"), 8000), Err(LoadRefusal::OverBudget));
    assert_eq!(
        loaded.admit(&id("c"), 4288),
        Ok(()),
        "exactly the budget fits"
    );
    assert_eq!(loaded.bytes(), BODY_BUDGET_BYTES);
    assert_eq!(
        loaded.admit(&id("a"), 8000),
        Ok(()),
        "a reload costs nothing"
    );
    assert_eq!(loaded.admit(&id("d"), 1), Err(LoadRefusal::OverBudget));
    assert_eq!(loaded.ids(), [&id("a"), &id("c")]);
}
