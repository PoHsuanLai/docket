//! The planner side of the live harness: the hijacked judge's cassette, the catalogue paths, the
//! scripted person, the attack turn and the report file.

use docket_core::Confirmer;
use docket_core::{
    Anchor, ConfirmAnswer, ConfirmDetail, ConfirmEnd, ConfirmOffer, ConfirmParts, ConfirmRequest,
    ContextSnapshot, Gesture, Here, Selection, TaintNote, TextTarget, TurnSource, TurnVia,
    UserTurn, Visible, WindowPrivacy,
};
use docket_live::attack_planner::{ask_of, planner_model_name};
use docket_live::engine::hijacked_judge_cassette;
use docket_live::person::Person;
use docket_live::report_file::write_report;
use docket_permissions_core::AskReason;
use docket_vocab::{LabelText, Seconds, TurnId};
use eval_core::Place;
use futures_util::FutureExt;
use porter_core::{AppName, Count};
use prov::{Label, Labelled, SessionId, UnixSeconds};
use std::collections::BTreeSet;

#[test]
fn the_hijacked_judge_has_an_entry_per_asker() {
    let text = hijacked_judge_cassette();
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).expect("json lines"))
        .collect();
    assert_eq!(lines.len(), 5, "a header and four entries");
    assert!(lines[1..].iter().all(|l| l["uses"] == "always"));
    let policy: serde_json::Value =
        serde_json::from_str(lines[1]["reply"]["v"].as_str().expect("text")).expect("policy");
    let actions = policy["actions"].as_array().expect("actions");
    assert!(
        actions
            .iter()
            .any(|a| a == "org.quire.Mail mail.message.send"),
        "{actions:?}"
    );
    assert!(actions.iter().any(|a| {
        a.as_str()
            .is_some_and(|s| s.starts_with("org.quire.Files "))
    }));
}

#[test]
fn the_catalogue_paths() {
    use docket_accept::catalog::{default_source, world_dir};
    use std::path::Path;
    assert_eq!(
        default_source(Path::new("/home/me/docket/eval")),
        Path::new("/home/me/stoker/catalog")
    );
    assert_eq!(
        default_source(Path::new("/home/me/rs-wt/x/docket/eval")),
        Path::new("/home/me/rs-wt/x/stoker/catalog")
    );
    assert_eq!(
        world_dir(Path::new("/s")),
        Path::new("/s/data/stoker/catalog")
    );
}

#[test]
fn the_planner_model_is_the_one_the_config_names_for_its_tier() {
    let config =
        "[ai.model.text]\nfast = \"local/a\"\nbalanced = \"local/b\"\nbest = \"local/c\"\n";
    assert_eq!(planner_model_name(config).as_deref(), Some("local/b"));
    assert_eq!(planner_model_name("[ai]\nlocal_only = \"on\"\n"), None);
    assert_eq!(planner_model_name("not toml ["), None);
}

fn sheet(app: &str, label: &str) -> ConfirmRequest {
    ConfirmRequest::new(ConfirmParts {
        id: prov::ConfirmId::parse("c-1").expect("id"),
        space: prov::SpaceId::parse("work").expect("space"),
        actor: prov::Actor::Unknown,
        app: AppName::parse(app).expect("app"),
        action: LabelText::parse(label).expect("label"),
        effect: prov::Effect::UndoableWrite,
        count: porter_core::Count(1),
        detail: ConfirmDetail::Plain,
        lines: vec![],
        why: vec![AskReason::Tainted],
        taint: TaintNote::Clean,
        offer: ConfirmOffer::OnceOnly,
        gesture: Gesture::Press,
        anchor: Anchor::Launcher,
        expires: Seconds(120),
    })
}

#[test]
fn the_scripted_person_allows_only_the_sheets_of_the_tasks_own_actions() {
    let mail = AppName::parse("org.quire.Mail").expect("app");
    let person = Person::saying_yes_to(BTreeSet::from([(mail.clone(), "Archive".to_owned())]));
    assert!(person.says_yes(&mail, "Archive"));
    assert!(!person.says_yes(&mail, "Send"));
    assert!(!person.says_yes(&AppName::parse("org.quire.Files").expect("app"), "Archive"));
    let asked = [
        ("org.quire.Mail", "Archive", true),
        ("org.quire.Mail", "Send", false),
        ("org.quire.Files", "Archive", false),
    ];
    for (app, label, yes) in asked {
        let answer = person
            .confirm(sheet(app, label))
            .now_or_never()
            .expect("at once");
        match (yes, answer) {
            (true, ConfirmAnswer::Allowed { .. }) => {}
            (false, ConfirmAnswer::Ended(ConfirmEnd::Refused)) => {}
            (_, other) => panic!("{app} {label}: {other:?}"),
        }
    }
    assert_eq!(
        person.requests().len(),
        3,
        "every sheet is kept for the trace"
    );
}

#[test]
fn an_attack_turn_is_asked_from_the_place_the_case_names() {
    let app = AppName::parse("org.quire.Mail").expect("app");
    let place = Place {
        app: app.clone(),
        window: ContextSnapshot {
            app: app.clone(),
            window: Labelled {
                value: "org.quire.Mail".to_owned(),
                label: Label::trusted_user(),
            },
            here: Here::Nowhere,
            selection: Selection::Nothing,
            visible: Visible {
                kind: None,
                items: vec![],
                total: Count(0),
            },
            text_target: TextTarget::None,
            privacy: WindowPrivacy::Normal,
        },
    };
    let turn = UserTurn {
        id: TurnId(1),
        text: "summarise this thread".to_owned(),
        at: UnixSeconds(1),
        from: TurnSource::Launcher,
        via: TurnVia::Typed,
        mode: None,
    };
    let ask = ask_of(SessionId::parse("s-1").expect("session"), turn, &place).expect("ask");
    assert_eq!(ask.app, Some(app), "the companion reads this app's window");
    assert_eq!(ask.turn.text, "summarise this thread");
}

#[test]
fn a_report_is_written_into_a_directory_that_is_made_and_a_failure_names_the_path() {
    let root = tempfile::tempdir().expect("dir");
    let report = root.path().join("eval/reports/live-1.md");
    write_report(&report, "numbers\n").expect("written");
    assert_eq!(std::fs::read_to_string(&report).expect("read"), "numbers\n");
    // A file where the directory should be: the error says where.
    let blocked = report.join("deeper.md");
    let why = write_report(&blocked, "x")
        .expect_err("a file is not a directory")
        .to_string();
    assert!(why.contains("live-1.md"), "{why}");
}
