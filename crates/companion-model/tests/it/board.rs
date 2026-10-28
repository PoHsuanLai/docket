//! The board: who takes which job, the two channels, one writer per thing, the breaker, the wait
//! for a model, and the records a restart rebuilds it from. `step` is pure, so each row is a
//! sequence of events and what came out.

use crate::helper::{chosen, draft, hid};
use crate::support::*;
use companion_model::helper::*;
use prov::{HelperId, SessionId, TaskId};

pub(crate) fn create(board: Board, name: &str, model: HelperModel) -> (Board, HelperId) {
    let (board, fx) = step(board, BoardEvent::Create(Box::new(draft(name, model))));
    let id = fx
        .iter()
        .find_map(|e| match e {
            BoardEffect::Created(id) => Some(id.clone()),
            _ => None,
        })
        .expect("a created card");
    (board, id)
}

pub(crate) fn goal(text: &str) -> GoalText {
    GoalText::parse(text).expect("goal")
}

pub(crate) fn job_for(to: &HelperId, text: &str, origin: GoalOrigin) -> BoardEvent {
    BoardEvent::Post(Box::new(NewJob {
        to: to.clone(),
        goal: goal(text),
        origin,
        giver: None,
        owns: vec![],
        with: vec![],
    }))
}

fn owning(to: &HelperId, text: &str, things: Vec<prov::EntityId>) -> BoardEvent {
    BoardEvent::Post(Box::new(NewJob {
        to: to.clone(),
        goal: goal(text),
        origin: GoalOrigin::Person,
        giver: None,
        owns: things,
        with: vec![],
    }))
}

pub(crate) fn posted(fx: &[BoardEffect]) -> JobId {
    fx.iter()
        .find_map(|e| match e {
            BoardEffect::Posted(id) => Some(*id),
            _ => None,
        })
        .expect("a posted job")
}

pub(crate) fn launched(fx: &[BoardEffect]) -> Vec<(HelperId, JobId)> {
    fx.iter()
        .filter_map(|e| match e {
            BoardEffect::Launch(l) => Some((l.helper.clone(), l.job)),
            _ => None,
        })
        .collect()
}

pub(crate) fn said(fx: &[BoardEffect]) -> Vec<Notice> {
    fx.iter()
        .filter_map(|e| match e {
            BoardEffect::Say { notice, .. } => Some(notice.clone()),
            _ => None,
        })
        .collect()
}

pub(crate) fn task(n: u64) -> TaskId {
    TaskId::parse(&format!("t-{n}")).expect("task")
}

fn result(n: u64) -> ResultHandle {
    ResultHandle {
        task: task(n),
        session: SessionId::parse(&format!("s-{n}")).expect("session"),
    }
}

pub(crate) fn state_of(board: &Board, helper: &HelperId) -> HelperState {
    board.card(helper).expect("card").state.clone()
}

#[test]
fn two_helpers_take_two_jobs_at_once_each_its_own() {
    let (board, research) = create(Board::new(), "Research", chosen());
    let (board, inbox) = create(board, "Inbox", chosen());
    let (board, fx) = step(
        board,
        job_for(&research, "compare the offers", GoalOrigin::Person),
    );
    let first = posted(&fx);
    assert_eq!(launched(&fx), [(research.clone(), first)]);
    let (board, fx) = step(board, job_for(&inbox, "sort the mail", GoalOrigin::Person));
    let second = posted(&fx);
    assert_eq!(launched(&fx), [(inbox.clone(), second)]);

    let (board, _) = step(
        board,
        BoardEvent::Started {
            job: first,
            task: task(1),
        },
    );
    let (board, _) = step(
        board,
        BoardEvent::Started {
            job: second,
            task: task(2),
        },
    );
    assert_eq!(board.running(&research).map(|j| j.id), Some(first));
    assert_eq!(board.running(&inbox).map(|j| j.id), Some(second));
    assert!(matches!(
        state_of(&board, &research),
        HelperState::Working(_)
    ));
    assert!(matches!(state_of(&board, &inbox), HelperState::Working(_)));

    // A third job for a busy helper waits; it is not taken beside the first.
    let (board, fx) = step(
        board,
        job_for(&research, "and the fourth", GoalOrigin::Person),
    );
    assert!(launched(&fx).is_empty());
    let third = posted(&fx);
    assert_eq!(
        board.job(third).map(|j| j.status.clone()),
        Some(JobStatus::Open)
    );

    // When the first is done, the helper takes the one that waited.
    let (board, fx) = step(
        board,
        BoardEvent::Finished {
            job: first,
            result: result(1),
        },
    );
    assert_eq!(launched(&fx), [(research.clone(), third)]);
    assert!(matches!(
        board.job(first).map(|j| &j.status),
        Some(JobStatus::Done(_))
    ));
}

#[test]
fn a_helper_with_no_model_waits_to_be_given_one_and_never_borrows_another() {
    let (board, research) = create(Board::new(), "Research", HelperModel::Unset);
    let (board, fx) = step(board, job_for(&research, "compare", GoalOrigin::Person));
    let job = posted(&fx);
    assert!(launched(&fx).is_empty(), "nothing runs without a model");
    assert_eq!(said(&fx), [Notice::WaitsForModel]);
    assert_eq!(
        state_of(&board, &research),
        HelperState::Waiting(Need::ChooseModel)
    );
    assert_eq!(
        board.job(job).map(|j| j.status.clone()),
        Some(JobStatus::Blocked(Need::ChooseModel))
    );

    // Said once: a second job and a second try add no second sentence.
    let (board, fx) = step(board, job_for(&research, "and this", GoalOrigin::Person));
    assert!(said(&fx).is_empty() && launched(&fx).is_empty());
    let (board, fx) = step(board, BoardEvent::Take(research.clone()));
    assert!(said(&fx).is_empty() && launched(&fx).is_empty());

    // The person chooses; the first job starts.
    let (board, fx) = step(
        board,
        BoardEvent::Edit {
            helper: research.clone(),
            draft: Box::new(draft("Research", chosen())),
        },
    );
    assert_eq!(launched(&fx), [(research.clone(), job)]);
    assert_eq!(board.running(&research).map(|j| j.id), Some(job));
}

#[test]
fn only_the_persons_words_or_a_clean_planners_words_become_an_instruction() {
    let (board, research) = create(Board::new(), "Research", chosen());
    let table = [
        (GoalOrigin::Person, Ok(GoalSource::Person)),
        (
            GoalOrigin::Planner(PlannerSaw::Nothing),
            Ok(GoalSource::PlannerOnTrusted),
        ),
        (
            GoalOrigin::Planner(PlannerSaw::Untrusted),
            Err(JobRefusal::UntrustedGoal),
        ),
        (GoalOrigin::ConfirmedBrief, Ok(GoalSource::Brief)),
    ];
    for (origin, want) in table {
        assert_eq!(admit_goal(origin), want, "{origin:?}");
    }

    let (board, fx) = step(
        board,
        job_for(
            &research,
            "send what the mail said",
            GoalOrigin::Planner(PlannerSaw::Untrusted),
        ),
    );
    assert_eq!(fx, [BoardEffect::JobRefused(JobRefusal::UntrustedGoal)]);
    assert!(board.jobs().is_empty(), "nothing was made");

    let (board, fx) = step(
        board,
        job_for(
            &research,
            "tidy the drafts",
            GoalOrigin::Planner(PlannerSaw::Nothing),
        ),
    );
    let made = board.job(posted(&fx)).expect("job");
    assert_eq!(made.source, GoalSource::PlannerOnTrusted);
    assert!(
        JobRefusal::UntrustedGoal
            .plain()
            .contains("tell the helper yourself")
    );
    assert!(JobRefusal::UntrustedGoal.plain().contains("check it first"));
}

#[test]
fn a_job_for_nobody_is_refused() {
    let (board, _) = create(Board::new(), "Research", chosen());
    let (_, fx) = step(board, job_for(&hid(99), "anything", GoalOrigin::Person));
    assert_eq!(fx, [BoardEffect::JobRefused(JobRefusal::NoSuchHelper)]);
}

#[test]
fn a_thing_has_one_writer_while_its_job_is_live() {
    let draft_thread = entity("mail.thread", "t1");
    let (board, research) = create(Board::new(), "Research", chosen());
    let (board, inbox) = create(board, "Inbox", chosen());
    let (board, fx) = step(
        board,
        owning(&research, "draft the reply", vec![draft_thread.clone()]),
    );
    let first = posted(&fx);

    // Inbox may not take the same thing while Research's job is live.
    let (board, fx) = step(
        board,
        owning(&inbox, "edit the same", vec![draft_thread.clone()]),
    );
    assert_eq!(
        fx,
        [BoardEffect::JobRefused(JobRefusal::Owned(
            board.card(&research).expect("card").name.clone()
        ))]
    );
    assert_eq!(
        board
            .owner_of(&draft_thread, None)
            .map(|(c, j)| (c.id.clone(), j.id)),
        Some((research.clone(), first))
    );
    assert!(board.owner_of(&draft_thread, Some(first)).is_none());
    assert_eq!(
        JobRefusal::Owned(name_of("Research")).plain(),
        "Research is working on this."
    );

    // When it is done the thing is free again.
    let (board, _) = step(
        board,
        BoardEvent::Finished {
            job: first,
            result: result(1),
        },
    );
    assert!(board.owner_of(&draft_thread, None).is_none());
    let (_, fx) = step(board, owning(&inbox, "edit it now", vec![draft_thread]));
    assert!(matches!(fx.first(), Some(BoardEffect::Save(_))));
}

fn name_of(text: &str) -> HelperName {
    HelperName::parse(text).expect("name")
}

#[test]
fn three_refusals_in_a_row_pause_the_helper_and_say_so_once() {
    let (board, research) = create(Board::new(), "Research", chosen());
    let (board, fx) = step(board, job_for(&research, "go", GoalOrigin::Person));
    let job = posted(&fx);
    let (mut board, _) = step(board, BoardEvent::Started { job, task: task(1) });
    let call = |board: Board, verdict| {
        step(
            board,
            BoardEvent::Called {
                helper: research.clone(),
                verdict,
            },
        )
    };
    // Two refusals and an allowed call: the count starts again.
    for verdict in [
        CallVerdict::Denied,
        CallVerdict::Denied,
        CallVerdict::Allowed,
    ] {
        let (next, fx) = call(board, verdict);
        assert!(said(&fx).is_empty());
        board = next;
    }
    for _ in 0..2 {
        let (next, fx) = call(board, CallVerdict::Denied);
        assert!(said(&fx).is_empty());
        board = next;
    }
    let (board, fx) = call(board, CallVerdict::Denied);
    assert_eq!(said(&fx), [Notice::Paused(PauseWhy::Denied3)]);
    assert_eq!(
        state_of(&board, &research),
        HelperState::Paused(PauseWhy::Denied3)
    );
    // Its running task is stopped and the job is open again, with nothing held against it.
    assert!(
        fx.iter()
            .any(|e| matches!(e, BoardEffect::Stop(at) if at.job == job))
    );
    let reopened = board.job(job).expect("job");
    assert_eq!(reopened.status, JobStatus::Open);
    assert!(reopened.failed.is_none());

    // A paused helper takes no job; resuming it gives it the open one.
    let (board, fx) = step(board, BoardEvent::Take(research.clone()));
    assert!(launched(&fx).is_empty());
    let (board, fx) = step(board, BoardEvent::Resume(research.clone()));
    assert_eq!(launched(&fx), [(research.clone(), job)]);
    assert_eq!(board.running(&research).map(|j| j.id), Some(job));
}

#[test]
fn three_failed_jobs_in_a_row_pause_the_helper_and_a_finished_one_starts_the_count_again() {
    let (board, research) = create(Board::new(), "Research", chosen());
    let (mut board, fx) = step(board, job_for(&research, "one", GoalOrigin::Person));
    let mut job = posted(&fx);
    let fail = |board: Board, job: JobId| {
        let (board, _) = step(
            board,
            BoardEvent::Started {
                job,
                task: task(job.0),
            },
        );
        step(
            board,
            BoardEvent::Failed {
                job,
                fault: JobFault::Crashed,
            },
        )
    };
    for round in 0..2 {
        let (next, fx) = fail(board, job);
        assert_eq!(
            said(&fx),
            [Notice::Failed(JobFault::Crashed)],
            "round {round}"
        );
        // The job is open again with its reason; it is not retried by itself.
        let open = next.job(job).expect("job");
        assert_eq!(open.status, JobStatus::Open);
        assert_eq!(open.failed, Some(JobFault::Crashed));
        assert!(launched(&fx).is_empty());
        let (next, fx) = step(next, BoardEvent::Take(research.clone()));
        assert!(
            launched(&fx).is_empty(),
            "a job that failed waits to be asked again"
        );
        let (next, fx) = step(next, BoardEvent::Retry(research.clone()));
        assert_eq!(launched(&fx), [(research.clone(), job)]);
        board = next;
    }
    let (board, fx) = fail(board, job);
    assert_eq!(
        said(&fx),
        [
            Notice::Failed(JobFault::Crashed),
            Notice::Paused(PauseWhy::Failed3)
        ]
    );
    assert_eq!(
        state_of(&board, &research),
        HelperState::Paused(PauseWhy::Failed3)
    );

    // After a resume, a job that finishes resets the count: two more failures do not pause.
    let (board, _) = step(board, BoardEvent::Resume(research.clone()));
    let (board, _) = step(
        board,
        BoardEvent::Started {
            job,
            task: task(50),
        },
    );
    let (board, _) = step(
        board,
        BoardEvent::Finished {
            job,
            result: result(50),
        },
    );
    let (mut board, fx) = step(board, job_for(&research, "two", GoalOrigin::Person));
    job = posted(&fx);
    for _ in 0..2 {
        let (next, fx) = fail(board, job);
        assert!(!said(&fx).contains(&Notice::Paused(PauseWhy::Failed3)));
        let (next, _) = step(next, BoardEvent::Retry(research.clone()));
        board = next;
    }
    assert!(!matches!(
        state_of(&board, &research),
        HelperState::Paused(_)
    ));
}
