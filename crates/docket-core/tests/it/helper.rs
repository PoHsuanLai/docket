//! Helpers' cards: the person's words, a model that is never defaulted, and the slugs that go to
//! disk and over the bus.

use crate::support::*;
use docket_core::ActionMatch;
use docket_core::helper::*;
use porter_core::{AccountId, Count, ModelId};
use porter_infer::ModelRef;
use prov::{Effect, HelperId, TaskId};
use std::collections::BTreeSet;

pub(crate) fn hid(n: u64) -> HelperId {
    serde_json::from_str(&format!("\"h-{n}\"")).expect("helper id")
}

pub(crate) fn name(text: &str) -> HelperName {
    HelperName::parse(text).expect("name")
}

pub(crate) fn chosen() -> HelperModel {
    HelperModel::Named(ModelRef {
        account: AccountId::parse("local").expect("account"),
        model: ModelId::parse("tiny").expect("model"),
    })
}

pub(crate) fn draft(helper: &str, model: HelperModel) -> CardDraft {
    CardDraft {
        name: name(helper),
        role: Role::parse("Reads the mail and says what needs me.").expect("role"),
        kind: HelperKind::Native,
        space: space("work"),
        model,
        reach: crate::task_policy::policy(),
        budget: HelperBudget { steps: Count(20) },
    }
}

pub(crate) fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("json")
}

#[test]
fn a_name_and_a_role_are_the_persons_words_and_bounded() {
    let long_name = "n".repeat(41);
    let long_role = "r".repeat(601);
    let names: Vec<(&str, &str, Result<&str, HelperTextFault>)> = vec![
        ("a plain name", "Research", Ok("Research")),
        ("trimmed", "  Inbox \n", Ok("Inbox")),
        ("empty", "  ", Err(HelperTextFault::Empty)),
        (
            "too long",
            long_name.as_str(),
            Err(HelperTextFault::TooLong),
        ),
        ("two lines", "In\nbox", Err(HelperTextFault::NotText)),
    ];
    for (what, text, want) in names {
        let got = HelperName::parse(text);
        assert_eq!(
            got.as_ref().map(HelperName::as_str).map_err(|e| *e),
            want,
            "{what}"
        );
    }
    assert!(Role::parse("Reads the mail.\nSays what needs me.").is_ok());
    assert_eq!(Role::parse(""), Err(HelperTextFault::Empty));
    assert_eq!(Role::parse(&long_role), Err(HelperTextFault::TooLong));
    assert_eq!(Role::parse("a\u{7}b"), Err(HelperTextFault::NotText));
}

#[test]
fn a_name_that_is_not_one_does_not_come_off_the_wire() {
    assert!(serde_json::from_str::<HelperName>("\"\"").is_err());
    assert!(serde_json::from_str::<Role>("\"   \"").is_err());
    let read: HelperName = serde_json::from_str("\"Research\"").expect("name");
    assert_eq!(read.as_str(), "Research");
}

#[test]
fn the_card_words_are_stable_slugs() {
    assert_eq!(json(&HelperKind::Native), r#""native""#);
    assert_eq!(json(&HelperModel::Unset), r#"{"kind":"unset"}"#);
    assert_eq!(json(&HelperState::Idle), r#"{"kind":"idle"}"#);
    assert_eq!(
        json(&HelperState::Waiting(Need::ChooseModel)),
        r#"{"kind":"waiting","v":"choose_model"}"#
    );
    let why = [
        (PauseWhy::ByPerson, "by_person"),
        (PauseWhy::Denied3, "denied3"),
        (PauseWhy::Failed3, "failed3"),
    ];
    for (why, slug) in why {
        assert_eq!(
            json(&HelperState::Paused(why)),
            format!(r#"{{"kind":"paused","v":"{slug}"}}"#)
        );
    }
    assert_eq!(
        json(&GoalSource::PlannerOnTrusted),
        r#""planner_on_trusted""#
    );
    assert_eq!(json(&JobId(7)), "7");
}

#[test]
fn a_new_card_is_idle_and_remembers_what_the_person_confirmed() {
    let card = HelperCard::new(hid(1), draft("Research", HelperModel::Unset));
    assert_eq!(card.state, HelperState::Idle);
    assert_eq!(
        card.model,
        HelperModel::Unset,
        "no model is ever chosen for the person"
    );
    assert_eq!(card.name.as_str(), "Research");
    assert!(card.working_on().is_none());
    let again: HelperCard = serde_json::from_str(&json(&card)).expect("card");
    assert_eq!(again, card);
}

#[test]
fn only_a_free_or_model_less_helper_takes_a_job() {
    let task = TaskId::parse("t-1").expect("task");
    let table = [
        (HelperState::Idle, true),
        (HelperState::Waiting(Need::ChooseModel), true),
        (
            HelperState::Working(TaskRef {
                job: JobId(1),
                task,
            }),
            false,
        ),
        (HelperState::Paused(PauseWhy::ByPerson), false),
        (HelperState::Paused(PauseWhy::Denied3), false),
        (HelperState::Paused(PauseWhy::Failed3), false),
    ];
    for (state, takes) in table {
        assert_eq!(state.takes_jobs(), takes, "{state:?}");
    }
}

#[test]
fn a_goal_is_words_that_fit_and_its_debug_shows_no_words() {
    assert!(GoalText::parse("compare the three offers").is_ok());
    assert_eq!(GoalText::parse(" "), Err(GoalFault::Empty));
    assert_eq!(GoalText::parse(&"g".repeat(1_001)), Err(GoalFault::TooLong));
    // A goal of fewer characters that is too many bytes is too long too.
    assert_eq!(
        GoalText::parse(&"\u{4e2d}".repeat(700)),
        Err(GoalFault::TooLong)
    );
    let shown = format!("{:?}", GoalText::parse("secret plan").expect("goal"));
    assert!(!shown.contains("secret"), "{shown}");
}

#[test]
fn a_reach_is_the_named_apps_up_to_the_chosen_ceiling_and_nothing_else() {
    let mail = manifest("org.quire.Mail", vec![]);
    let files = manifest("org.quire.Files", vec![]);
    let installed = [&mail, &files];
    let policy = reach_policy(
        &[app("org.quire.Mail")],
        Ceiling::Change,
        &space("work"),
        &installed,
    )
    .expect("a reach");
    assert_eq!(policy.ceiling, Effect::UndoableWrite);
    assert_eq!(
        policy.actions,
        BTreeSet::from([ActionMatch::AppUpTo(
            app("org.quire.Mail"),
            Effect::UndoableWrite
        )])
    );
    assert!(
        policy.recipients.is_empty() && policy.destinations.is_empty() && policy.paths.is_empty()
    );
    assert_eq!(
        reach_lines(&policy),
        ["org.quire.Mail: look at things and change them (changes can be undone)"]
    );

    let ceilings = [
        (Ceiling::Look, Effect::Read),
        (Ceiling::Change, Effect::UndoableWrite),
        (Ceiling::Send, Effect::Outbound),
    ];
    for (ceiling, effect) in ceilings {
        assert_eq!(ceiling.effect(), effect);
    }
    assert_eq!(
        reach_policy(&[], Ceiling::Look, &space("work"), &installed),
        Err(ReachFault::NoApps)
    );
    assert_eq!(
        reach_policy(
            &[app("org.quire.Mail"), app("org.quire.Chat")],
            Ceiling::Look,
            &space("work"),
            &installed
        ),
        Err(ReachFault::NoSuchApp("org.quire.Chat".to_owned()))
    );
}

#[test]
fn two_helpers_that_run_the_same_program_do_not_share_a_grant_caller() {
    use docket_core::{GrantCaller, ProgramName, holds_standing};
    let program = ProgramName::parse("claude-code").expect("program");
    let first = GrantCaller::AcpHelper {
        program: program.clone(),
        helper: hid(1),
    };
    let second = GrantCaller::AcpHelper {
        program: program.clone(),
        helper: hid(2),
    };
    assert_ne!(first, second);
    assert_ne!(first, GrantCaller::AcpAgent(program));
    assert_eq!(first.kind(), prov::ActorKind::Acp);
    assert!(
        holds_standing(&first),
        "it holds standing grants like its program does"
    );
    let back: GrantCaller = serde_json::from_str(&json(&first)).expect("caller");
    assert_eq!(back, first);
    // A grant stored for the program as a whole still reads.
    let old = r#"{"kind":"acp_agent","v":"claude-code"}"#;
    assert!(matches!(
        serde_json::from_str::<GrantCaller>(old),
        Ok(GrantCaller::AcpAgent(_))
    ));
}
