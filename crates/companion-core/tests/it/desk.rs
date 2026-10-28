//! The desks' decisions over words and an empty board: what is refused, and why.

use companion_core::{
    PROPOSED_STEPS, PlannerJob, Refusal, RoutineWho, RoutineWish, helper_form, person_job,
    planner_job, routine_wish,
};
use companion_model::helper::{Board, Ceiling, GoalOrigin, JobRefusal, ReachFault};
use prov::{HelperId, SpaceId, TaskId};

fn space() -> SpaceId {
    SpaceId::parse("work").expect("space")
}

fn helper() -> HelperId {
    serde_json::from_str("\"h-1\"").expect("helper id")
}

#[test]
fn a_job_for_a_helper_that_is_not_on_the_board_is_refused() {
    let board = Board::new();
    assert_eq!(
        person_job(&board, &helper(), "Check the mail", Vec::new()),
        Err(Refusal::Job(JobRefusal::NoSuchHelper))
    );
    let giver = TaskId::parse("t-1").expect("task");
    let gift = PlannerJob {
        helper: "research",
        space: &space(),
        goal: "Check the mail",
        after: None,
        giver: &giver,
        origin: GoalOrigin::Person,
    };
    assert_eq!(
        planner_job(&board, gift),
        Err(Refusal::Job(JobRefusal::NoSuchHelper))
    );
}

#[test]
fn a_proposed_routine_names_a_helper_that_exists() {
    let board = Board::new();
    let wish = RoutineWish {
        who: RoutineWho::Itself(&helper()),
        goal: "Check the mail",
        when: "daily 08:00",
        why: "You asked for it.",
    };
    assert_eq!(
        routine_wish(&board, wish),
        Err(Refusal::Job(JobRefusal::NoSuchHelper))
    );
}

#[test]
fn a_proposed_helper_is_a_form_of_its_own_planner_with_the_steps_it_is_proposed_with() {
    let apps = ["org.quire.Mail".to_owned()];
    let form = helper_form("Inbox", "Reads the mail", &apps, Ceiling::Look, space()).expect("form");
    assert_eq!(form.name.as_str(), "Inbox");
    assert_eq!(form.steps, PROPOSED_STEPS);
    assert_eq!(form.apps.len(), 1);
}

#[test]
fn a_proposed_helper_with_a_bad_name_or_app_is_refused() {
    assert_eq!(
        helper_form("", "Reads the mail", &[], Ceiling::Look, space()),
        Err(Refusal::BadNameOrRole)
    );
    let bad = ["Not An App".to_owned()];
    assert_eq!(
        helper_form("Inbox", "Reads the mail", &bad, Ceiling::Look, space()),
        Err(Refusal::Reach(ReachFault::NoSuchApp(
            "Not An App".to_owned()
        )))
    );
}
