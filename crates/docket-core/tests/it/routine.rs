//! Routines: a time fires one job, a missed time is skipped and recorded, three runs that do not
//! finish pause the routine and say so once, an event is a handle and never part of the goal, and
//! a job's end starts what waits for it. `step` is pure, so each row is a sequence of events.

use crate::helper::hid;
use crate::support::*;
use almanac_core::{EventRef, KindPattern, ReplicaId, Seq};
use docket_core::helper::{GoalText, HelperRecord, HelperRecordKind, JobFault, JobId};
use docket_core::routine::*;
use porter_core::Count;
use prov::HelperId;

const H: i64 = 3_600;
const DAY: i64 = 86_400;

fn goal() -> GoalText {
    GoalText::parse("Summarise my unread mail and list what needs an answer").expect("goal")
}

fn eight() -> Trigger {
    Trigger::At(Schedule::parse("daily 08:00").expect("schedule"))
}

fn made(
    book: RoutineBook,
    helper: &HelperId,
    trigger: Trigger,
    missed: Missed,
    now: i64,
) -> (RoutineBook, RoutineId) {
    let (book, fx) = step(
        book,
        RoutineEvent::Add {
            new: Box::new(NewRoutine {
                helper: helper.clone(),
                goal: goal(),
                trigger,
                budget: RunBudget { steps: Count(8) },
                missed,
            }),
            now: at(now),
            zone: Zone::UTC,
        },
    );
    let id = fx
        .iter()
        .find_map(|e| match e {
            RoutineEffect::Created(id) => Some(*id),
            _ => None,
        })
        .expect("a created routine");
    (book, id)
}

fn tick(book: RoutineBook, now: i64) -> (RoutineBook, Vec<RoutineEffect>) {
    step(
        book,
        RoutineEvent::Tick {
            now: at(now),
            zone: Zone::UTC,
        },
    )
}

fn runs(fx: &[RoutineEffect]) -> Vec<&RunRequest> {
    fx.iter()
        .filter_map(|e| match e {
            RoutineEffect::Run(run) => Some(run),
            _ => None,
        })
        .collect()
}

fn says(fx: &[RoutineEffect]) -> usize {
    fx.iter()
        .filter(|e| matches!(e, RoutineEffect::Say { .. }))
        .count()
}

fn outcomes(book: &RoutineBook, id: RoutineId) -> Vec<RunOutcome> {
    book.routine(id)
        .expect("routine")
        .runs
        .iter()
        .map(|run| run.outcome)
        .collect()
}

/// Posts the job a run asked for and returns the book with it.
fn posted(book: RoutineBook, routine: RoutineId, job: u64) -> RoutineBook {
    step(
        book,
        RoutineEvent::Posted {
            routine,
            job: JobId(job),
        },
    )
    .0
}

fn ended(book: RoutineBook, job: u64, end: JobEnd, now: i64) -> (RoutineBook, Vec<RoutineEffect>) {
    step(
        book,
        RoutineEvent::Ended {
            job: JobId(job),
            end,
            now: at(now),
        },
    )
}

fn event(n: u64) -> EventRef {
    EventRef {
        space: space("work"),
        replica: ReplicaId([1; 16]),
        seq: Seq(n),
    }
}

#[test]
fn schedules_read_plain_words_and_say_the_next_time() {
    let daily = Schedule::parse("Daily 8:00").expect("daily");
    assert_eq!(daily.plain(), "Every day at 8:00");
    assert_eq!(daily.calendar(), "*-*-* 08:00:00");
    assert_eq!(daily.next_after(at(7 * H), Zone::UTC), at(8 * H));
    assert_eq!(daily.next_after(at(8 * H), Zone::UTC), at(DAY + 8 * H));
    // Eight o'clock on a clock two hours east is six o'clock UTC.
    assert_eq!(daily.next_after(at(0), Zone(2 * 3600)), at(6 * H));

    // 1970-01-02 was a Friday: the next weekday time is Monday's.
    let weekdays = Schedule::parse("weekdays 08:00").expect("weekdays");
    assert_eq!(weekdays.calendar(), "Mon..Fri *-*-* 08:00:00");
    assert_eq!(
        weekdays.next_after(at(DAY + 9 * H), Zone::UTC),
        at(4 * DAY + 8 * H)
    );

    let every = Schedule::parse("every 2 hours").expect("every");
    assert_eq!(every.plain(), "Every 2 hours");
    assert_eq!(every.calendar(), "*-*-* 0/2:00:00");
    assert_eq!(every.next_after(at(3 * H), Zone::UTC), at(4 * H));
    assert_eq!(every.next_after(at(23 * H), Zone::UTC), at(DAY));
    assert_eq!(
        Schedule::parse("every hour").expect("hourly").plain(),
        "Every hour"
    );

    for bad in ["", "daily", "daily 25:00", "every 0 hours", "0 8 * * *"] {
        assert!(Schedule::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn a_due_time_fires_one_job_with_the_persons_words() {
    let (book, id) = made(
        RoutineBook::default(),
        &hid(1),
        eight(),
        Missed::Skip,
        7 * H,
    );
    let (book, fx) = tick(book, 7 * H + 59 * 60);
    assert!(runs(&fx).is_empty(), "not yet due");

    let (book, fx) = tick(book, 8 * H + 5);
    let asked = runs(&fx);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].helper, hid(1));
    assert_eq!(asked[0].goal, goal());
    assert_eq!(asked[0].event, None);
    // The run carries the routine's own budget, which the job is held to.
    assert_eq!(asked[0].budget, RunBudget { steps: Count(8) });
    assert_eq!(outcomes(&book, id), [RunOutcome::Started]);
    assert_eq!(
        book.routine(id).expect("routine").next_due,
        Some(at(DAY + 8 * H))
    );

    // The same time does not fire twice.
    let (_, fx) = tick(book, 8 * H + 30);
    assert!(runs(&fx).is_empty());
}

#[test]
fn a_time_missed_while_asleep_is_skipped_and_recorded() {
    let (book, id) = made(
        RoutineBook::default(),
        &hid(1),
        eight(),
        Missed::Skip,
        7 * H,
    );
    // The computer woke at ten.
    let (book, fx) = tick(book, 10 * H);
    assert!(runs(&fx).is_empty());
    let routine = book.routine(id).expect("routine");
    assert_eq!(outcomes(&book, id), [RunOutcome::Skipped]);
    assert_eq!(routine.runs[0].at, at(8 * H));
    assert_eq!(routine.next_due, Some(at(DAY + 8 * H)));
    assert_eq!(Missed::default(), Missed::Skip);
}

#[test]
fn a_time_missed_runs_once_on_wake_when_the_person_said_so() {
    let (book, id) = made(
        RoutineBook::default(),
        &hid(1),
        eight(),
        Missed::RunOnceOnWake,
        7 * H,
    );
    let (book, fx) = tick(book, 10 * H);
    assert_eq!(runs(&fx).len(), 1);
    let (book, fx) = tick(book, 10 * H + 30);
    assert!(runs(&fx).is_empty(), "once, not again");
    assert_eq!(outcomes(&book, id), [RunOutcome::Started]);
}

#[test]
fn a_restart_after_the_time_skips_it_too() {
    let (book, id) = made(
        RoutineBook::default(),
        &hid(1),
        eight(),
        Missed::Skip,
        7 * H,
    );
    let records = book.records();
    let back = RoutineBook::restore(records);
    let (back, fx) = tick(back, 9 * H);
    assert!(runs(&fx).is_empty());
    assert_eq!(outcomes(&back, id), [RunOutcome::Skipped]);
}

#[test]
fn a_run_whose_job_was_never_posted_did_not_run_after_a_restart() {
    let (book, id) = made(
        RoutineBook::default(),
        &hid(1),
        eight(),
        Missed::Skip,
        7 * H,
    );
    let book = tick(book, 8 * H).0;
    assert_eq!(book.running(), 1);
    let back = RoutineBook::restore(book.records());
    assert_eq!(outcomes(&back, id), [RunOutcome::Skipped]);
    assert_eq!(back.running(), 0);
}

#[test]
fn three_runs_that_do_not_finish_pause_the_routine_and_say_so_once() {
    let (mut book, id) = made(RoutineBook::default(), &hid(1), eight(), Missed::Skip, 0);
    let mut said = 0;
    for day in 0..4 {
        let now = day * DAY + 8 * H;
        let (b, fx) = tick(book, now);
        book = b;
        if runs(&fx).is_empty() {
            break;
        }
        book = posted(book, id, 10 + day as u64);
        let (b, fx) = ended(
            book,
            10 + day as u64,
            JobEnd::Failed(JobFault::NoAnswer),
            now + 60,
        );
        book = b;
        said += says(&fx);
    }
    assert_eq!(said, 1);
    assert_eq!(
        book.routine(id).expect("routine").state,
        RoutineState::Paused(RoutinePause::Failed3)
    );
    // A paused routine does not run, and a timer for it is not wanted.
    assert!(book.timers().is_empty());
    let (_, fx) = tick(book, 5 * DAY + 8 * H);
    assert!(runs(&fx).is_empty());
}

#[test]
fn a_run_that_finishes_ends_the_streak() {
    let (book, id) = made(RoutineBook::default(), &hid(1), eight(), Missed::Skip, 0);
    let mut book = book;
    let plan = [false, false, true, false, false];
    for (day, ok) in plan.into_iter().enumerate() {
        let now = day as i64 * DAY + 8 * H;
        book = tick(book, now).0;
        book = posted(book, id, day as u64 + 1);
        let end = if ok {
            JobEnd::Done
        } else {
            JobEnd::Failed(JobFault::Crashed)
        };
        book = ended(book, day as u64 + 1, end, now + 60).0;
    }
    assert_eq!(
        book.routine(id).expect("routine").state,
        RoutineState::Active
    );
}

#[test]
fn an_event_starts_a_job_whose_goal_is_the_persons_words_and_whose_event_is_a_handle() {
    let watch = Trigger::On(EventMatch {
        kind: KindPattern::parse("mail.*").expect("kind"),
        space: None,
        words: "New mail in Inbox".to_owned(),
    });
    let (book, id) = made(RoutineBook::default(), &hid(1), watch, Missed::Skip, 0);
    let saw = |book, kind: &str, n, space_name: &str| {
        let mut e = event(n);
        e.space = space(space_name);
        step(
            book,
            RoutineEvent::Saw {
                routine: id,
                event: e,
                kind: kind.to_owned(),
                now: at(50),
            },
        )
    };
    let (book, fx) = saw(book, "mail.received", 7, "work");
    let asked = runs(&fx);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].goal, goal());
    assert_eq!(asked[0].event.as_ref().map(|e| e.seq), Some(Seq(7)));
    // The event is on the run's record as a handle; the goal is untouched.
    let routine = book.routine(id).expect("routine");
    assert_eq!(routine.goal, goal());
    assert_eq!(routine.runs[0].event.as_ref().map(|e| e.seq), Some(Seq(7)));

    // Another kind of event does not start it.
    let (_, fx) = saw(book, "file.changed", 8, "work");
    assert!(runs(&fx).is_empty());
}

#[test]
fn a_job_ending_done_starts_what_waits_for_it_and_a_failure_does_not() {
    let after = Trigger::AfterJob(JobRef(JobId(5)));
    let (book, id) = made(RoutineBook::default(), &hid(2), after, Missed::Skip, 0);

    let (book, fx) = ended(book, 5, JobEnd::Failed(JobFault::OutOfSteps), 10);
    assert!(runs(&fx).is_empty());
    let (book, fx) = ended(book, 6, JobEnd::Done, 11);
    assert!(runs(&fx).is_empty(), "another job");
    let (book, fx) = ended(book, 5, JobEnd::Done, 12);
    let asked = runs(&fx);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].helper, hid(2));
    assert_eq!(outcomes(&book, id), [RunOutcome::Started]);
    assert!(book.awaited_jobs().is_empty(), "it has had its job's end");
    // The same end told again starts nothing more.
    let (_, fx) = ended(book, 5, JobEnd::Done, 13);
    assert!(runs(&fx).is_empty());
}

#[test]
fn a_helper_that_cannot_take_the_job_is_a_skipped_run_not_a_failure() {
    let (book, id) = made(
        RoutineBook::default(),
        &hid(1),
        eight(),
        Missed::Skip,
        7 * H,
    );
    let book = tick(book, 8 * H).0;
    let book = step(book, RoutineEvent::NotPosted(id)).0;
    assert_eq!(outcomes(&book, id), [RunOutcome::Skipped]);
    assert_eq!(book.routine(id).expect("routine").failed, 0);
    assert_eq!(book.running(), 0);
}

#[test]
fn only_the_last_twenty_runs_are_kept() {
    let every = Trigger::At(Schedule::parse("every hour").expect("schedule"));
    let (mut book, id) = made(RoutineBook::default(), &hid(1), every, Missed::Skip, 0);
    for hour in 1..=25 {
        book = tick(book, hour * H).0;
    }
    let kept = &book.routine(id).expect("routine").runs;
    assert_eq!(kept.len(), MOST_RUNS);
    assert_eq!(kept[0].at, at(6 * H));
}

#[test]
fn pause_resume_and_remove_move_the_book_and_its_records() {
    let (book, id) = made(RoutineBook::default(), &hid(1), eight(), Missed::Skip, 0);
    let (book, fx) = step(book, RoutineEvent::Pause(id));
    assert_eq!(fx.len(), 1);
    assert!(book.timers().is_empty());
    // Resumed at nine, its next time is tomorrow's: the eight o'clock in between is not run.
    let (book, _) = step(
        book,
        RoutineEvent::Resume {
            id,
            now: at(9 * H),
            zone: Zone::UTC,
        },
    );
    assert_eq!(book.timers().len(), 1);
    assert_eq!(
        book.routine(id).expect("routine").next_due,
        Some(at(DAY + 8 * H))
    );
    let (book, fx) = step(book, RoutineEvent::Remove(id));
    assert!(book.routines().is_empty());
    assert!(matches!(
        fx.as_slice(),
        [RoutineEffect::Save(RoutineRecord::Removed { .. })]
    ));
}

#[test]
fn a_routine_record_round_trips_under_its_own_kind() {
    let (book, id) = made(RoutineBook::default(), &hid(1), eight(), Missed::Skip, 0);
    let book = posted(tick(book, 8 * H).0, id, 1);
    let record = HelperRecord::Routine(Box::new(RoutineRecord::Kept(Box::new(
        book.routine(id).expect("routine").clone(),
    ))));
    assert_eq!(record.kind(), HelperRecordKind::Routine);
    assert_eq!(
        HelperRecordKind::of_slug("helpers.routine"),
        Some(HelperRecordKind::Routine)
    );
    let text = record.json().expect("json");
    assert_eq!(
        HelperRecord::from_json(HelperRecordKind::Routine, &text).expect("back"),
        record
    );
    let HelperRecord::Routine(inner) = record else {
        unreachable!("built as a routine");
    };
    let back = RoutineBook::restore([*inner]);
    assert_eq!(back, book);
}
