//! The routines on the wire: a form reads from the shell's JSON (a missed time is skipped unless
//! the form says otherwise), and a card shows the last runs newest first in plain words.

use companion_wire::{
    RoutineForm, RoutineShown, RunShown, WhenForm, routine_card, routine_line,
};
use docket_core::helper::{GoalText, HelperName, JobFault, JobId};
use docket_core::routine::*;
use porter_core::Count;
use prov::{HelperId, UnixSeconds};

fn helper() -> HelperId {
    serde_json::from_str("\"h-1\"").expect("helper id")
}

fn inbox() -> HelperName {
    HelperName::parse("Inbox").expect("name")
}

#[test]
fn a_form_without_a_missed_choice_skips_missed_times() {
    let text = r#"{"helper":"h-1","goal":"Tidy my mail","when":{"kind":"at","v":"daily 08:00"},"steps":5}"#;
    let form: RoutineForm = serde_json::from_str(text).expect("form");
    assert_eq!(form.missed, Missed::Skip);
    assert_eq!(form.when, WhenForm::At("daily 08:00".to_owned()));
}

#[test]
fn a_card_shows_its_runs_newest_first_and_says_why_one_did_not_finish() {
    let new = NewRoutine {
        helper: helper(),
        goal: GoalText::parse("Tidy my mail").expect("goal"),
        trigger: Trigger::At(Schedule::parse("every hour").expect("schedule")),
        budget: RunBudget { steps: Count(6) },
        missed: Missed::Skip,
    };
    let zone = Zone::UTC;
    let (book, _) = step(
        RoutineBook::default(),
        RoutineEvent::Add {
            new: Box::new(new),
            now: UnixSeconds(0),
            zone,
        },
    );
    let (book, _) = step(book, RoutineEvent::Tick { now: UnixSeconds(3_600), zone });
    let (book, _) = step(
        book,
        RoutineEvent::Posted {
            routine: RoutineId(1),
            job: JobId(4),
        },
    );
    let (book, _) = step(
        book,
        RoutineEvent::Ended {
            job: JobId(4),
            end: JobEnd::Failed(JobFault::OutOfSteps),
            now: UnixSeconds(3_700),
        },
    );
    // A day later: the time was missed.
    let (book, _) = step(book, RoutineEvent::Tick { now: UnixSeconds(90_000), zone });

    let routine = book.routine(RoutineId(1)).expect("routine");
    let card = routine_card(routine, inbox());
    assert_eq!(card.line.when, "Every hour");
    assert_eq!(card.line.state, RoutineShown::On);
    assert_eq!(card.steps, 6);
    let states: Vec<RunShown> = card.runs.iter().map(|run| run.state).collect();
    assert_eq!(states, [RunShown::Skipped, RunShown::DidNotFinish]);
    assert_eq!(
        card.runs[1].note.as_deref(),
        Some("It stopped: it used up its steps")
    );
    assert_eq!(card.runs[1].job, Some(JobId(4)));
    assert_eq!(routine_line(routine, inbox()).last, Some(card.runs[0].clone()));
}
