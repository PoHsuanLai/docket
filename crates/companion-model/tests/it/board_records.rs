//! The board after a restart and past the first jobs: posts between helpers, the records a
//! restart rebuilds it from, a job about an event, and agent-app helpers that wait to be signed in.

use crate::board::{create, goal, job_for, launched, posted, said, state_of, task};
use crate::helper::{chosen, draft};
use crate::support::*;
use companion_model::helper::*;
use prov::HelperId;

#[test]
fn removing_a_helper_stops_its_task_and_forgets_its_jobs() {
    let (board, research) = create(Board::new(), "Research", chosen());
    let (board, fx) = step(board, job_for(&research, "go", GoalOrigin::Person));
    let job = posted(&fx);
    let (board, _) = step(board, BoardEvent::Started { job, task: task(1) });
    let (board, fx) = step(board, BoardEvent::Remove(research.clone()));
    assert!(
        fx.iter()
            .any(|e| matches!(e, BoardEffect::Stop(at) if at.job == job))
    );
    assert!(
        fx.contains(&BoardEffect::Save(HelperRecord::Card(CardRecord::Removed(
            research.clone()
        ))))
    );
    assert!(board.cards().is_empty() && board.jobs().is_empty());
}

#[test]
fn a_post_goes_to_the_helper_it_is_for_once_and_never_back_to_its_sender() {
    let (board, research) = create(Board::new(), "Research", chosen());
    let (board, inbox) = create(board, "Inbox", chosen());
    let (board, third) = create(board, "Notes", chosen());
    let (board, fx) = step(
        board,
        BoardEvent::Hand {
            from: research.clone(),
            to: PostTo::Helper(inbox.clone()),
            parts: vec![PostPart::Result(JobId(1))],
            label: untrusted_mail("work"),
        },
    );
    let post = match fx.first() {
        Some(BoardEffect::Save(HelperRecord::Post(post))) => post.id,
        other => panic!("a saved post, not {other:?}"),
    };
    assert_eq!(board.pending_posts(&inbox).len(), 1);
    assert!(board.pending_posts(&research).is_empty());
    assert!(board.pending_posts(&third).is_empty());
    let (board, _) = step(
        board,
        BoardEvent::Delivered {
            post,
            helper: inbox.clone(),
        },
    );
    assert!(board.pending_posts(&inbox).is_empty());

    // To the group: every helper but the sender, each once.
    let (board, _) = step(
        board,
        BoardEvent::Hand {
            from: research.clone(),
            to: PostTo::Group,
            parts: vec![PostPart::Thing(entity("mail.thread", "t2"))],
            label: untrusted_mail("work"),
        },
    );
    assert_eq!(board.pending_posts(&inbox).len(), 1);
    assert_eq!(board.pending_posts(&third).len(), 1);
    assert!(board.pending_posts(&research).is_empty());
}

#[test]
fn a_restart_rebuilds_the_board_from_what_it_wrote_and_nothing_is_still_running() {
    let mut records = Vec::new();
    let mut board = Board::new();
    let mut apply = |board: Board, event| {
        let (board, fx) = step(board, event);
        records.extend(fx.into_iter().filter_map(|e| match e {
            BoardEffect::Save(record) => Some(record),
            _ => None,
        }));
        board
    };
    board = apply(
        board,
        BoardEvent::Create(Box::new(draft("Research", chosen()))),
    );
    board = apply(
        board,
        BoardEvent::Create(Box::new(draft("Inbox", HelperModel::Unset))),
    );
    let research = board.cards()[0].id.clone();
    let inbox = board.cards()[1].id.clone();
    board = apply(board, job_for(&research, "compare", GoalOrigin::Person));
    let running = board.jobs()[0].id;
    board = apply(
        board,
        BoardEvent::Started {
            job: running,
            task: task(1),
        },
    );
    board = apply(
        board,
        job_for(&inbox, "sort", GoalOrigin::Planner(PlannerSaw::Nothing)),
    );
    let blocked = board.jobs()[1].id;
    board = apply(
        board,
        job_for(&research, "and then this", GoalOrigin::Person),
    );
    let finished_first = board.jobs()[2].id;
    assert!(matches!(
        state_of(&board, &research),
        HelperState::Working(_)
    ));
    assert_eq!(
        board.job(blocked).map(|j| j.status.clone()),
        Some(JobStatus::Blocked(Need::ChooseModel))
    );

    let back = Board::restore(records.clone());
    assert_eq!(back.cards().len(), 2);
    assert_eq!(state_of(&back, &research), HelperState::Idle);
    assert_eq!(state_of(&back, &inbox), HelperState::Idle);
    assert_eq!(
        back.job(running).map(|j| j.status.clone()),
        Some(JobStatus::Open)
    );
    assert_eq!(
        back.job(blocked).map(|j| j.status.clone()),
        Some(JobStatus::Open)
    );
    assert_eq!(
        back.job(blocked).map(|j| j.source),
        Some(GoalSource::PlannerOnTrusted)
    );
    assert_eq!(
        back.job(finished_first).map(|j| j.status.clone()),
        Some(JobStatus::Open)
    );

    // The newest record of a thing wins, a removed card stays removed, and ids go on from the
    // highest one seen.
    records.push(HelperRecord::Card(CardRecord::Removed(inbox.clone())));
    let back = Board::restore(records);
    assert!(back.card(&inbox).is_none());
    assert!(
        back.jobs().iter().all(|j| j.owner == research),
        "the removed helper's job is dropped"
    );
    let (_, again) = create(back, "Notes", chosen());
    assert_ne!(again, research);
    assert_ne!(again, inbox);
}

#[test]
fn records_keep_their_tags_and_read_back_from_their_payload() {
    for kind in HelperRecordKind::ALL {
        assert_eq!(HelperRecordKind::of_slug(kind.slug()), Some(kind));
    }
    assert_eq!(HelperRecordKind::Card.slug(), "helpers.card");
    assert_eq!(HelperRecordKind::of_slug("helpers.other"), None);
    let (board, research) = create(Board::new(), "Research", chosen());
    let (_, fx) = step(board, job_for(&research, "go", GoalOrigin::Person));
    for record in fx.iter().filter_map(|e| match e {
        BoardEffect::Save(r) => Some(r),
        _ => None,
    }) {
        let text = record.json().expect("json");
        let back = HelperRecord::from_json(record.kind(), &text).expect("record");
        assert_eq!(&back, record);
    }
}

#[test]
fn a_job_about_an_event_carries_the_event_as_a_handle_and_keeps_the_goal_the_persons() {
    let (board, inbox) = create(Board::default(), "Inbox", chosen());
    let about = almanac_core::EventRef {
        space: space("work"),
        replica: almanac_core::ReplicaId([2; 16]),
        seq: almanac_core::Seq(3),
    };
    let new = NewJob {
        to: inbox,
        goal: goal("Tidy the new mail"),
        origin: GoalOrigin::Person,
        giver: None,
        owns: vec![],
        with: vec![],
    };
    let (board, fx) = step(
        board,
        BoardEvent::PostRun {
            job: Box::new(new),
            about: Some(about.clone()),
            steps: porter_core::Count(4),
        },
    );
    let job = board.job(posted(&fx)).expect("job");
    assert_eq!(job.about, Some(about));
    assert_eq!(job.steps, Some(porter_core::Count(4)));
    assert_eq!(job.goal.as_str(), "Tidy the new mail");
}

fn create_app(board: Board, name: &str) -> (Board, HelperId) {
    let draft = CardDraft {
        kind: HelperKind::AgentApp(prov::AgentProgram::parse("claude-code").expect("program")),
        ..draft(name, HelperModel::Unset)
    };
    let (board, fx) = step(board, BoardEvent::Create(Box::new(draft)));
    let id = fx
        .iter()
        .find_map(|e| match e {
            BoardEffect::Created(id) => Some(id.clone()),
            _ => None,
        })
        .expect("a created card");
    (board, id)
}

#[test]
fn an_agent_app_helper_needs_no_model_because_the_app_brings_its_own() {
    let (board, coder) = create_app(Board::new(), "Coder");
    let (board, fx) = step(board, job_for(&coder, "tidy the repo", GoalOrigin::Person));
    let job = posted(&fx);
    assert_eq!(launched(&fx), [(coder.clone(), job)]);
    assert!(said(&fx).is_empty(), "no wait for a model: {fx:?}");
    assert_eq!(
        board.job(job).map(|j| j.status.clone()),
        Some(JobStatus::Taken(coder))
    );
}

#[test]
fn an_app_that_wants_signing_in_makes_the_helper_wait_once_and_fails_nothing() {
    let (board, coder) = create_app(Board::new(), "Coder");
    let (board, fx) = step(board, job_for(&coder, "tidy the repo", GoalOrigin::Person));
    let job = posted(&fx);
    let (board, _) = step(board, BoardEvent::Started { job, task: task(1) });
    let (board, fx) = step(board, BoardEvent::NeedsSignIn(job));
    assert_eq!(state_of(&board, &coder), HelperState::Waiting(Need::SignIn));
    assert_eq!(
        board.job(job).map(|j| (j.status.clone(), j.failed)),
        Some((JobStatus::Blocked(Need::SignIn), None)),
        "blocked, not failed"
    );
    assert_eq!(said(&fx), [Notice::WaitsForSignIn]);
    assert!(
        Notice::WaitsForSignIn
            .plain(&board.card(&coder).expect("card").name)
            .contains("sign in")
    );

    // The helper takes no job on its own while it waits, only when the person says try again.
    let (board, fx) = step(board, BoardEvent::Take(coder.clone()));
    assert!(launched(&fx).is_empty() && said(&fx).is_empty(), "{fx:?}");
    let (board, fx) = step(board, BoardEvent::Retry(coder.clone()));
    assert_eq!(launched(&fx), [(coder.clone(), job)]);
    let (board, _) = step(board, BoardEvent::Started { job, task: task(2) });
    // Asked again at the person's word and still not signed in, it says so again.
    let (board, fx) = step(board, BoardEvent::NeedsSignIn(job));
    assert_eq!(said(&fx), [Notice::WaitsForSignIn]);

    // Three of these in a row are not three failed jobs: the helper is never paused for them.
    let mut board = board;
    for n in 3..7 {
        board = step(board, BoardEvent::Retry(coder.clone())).0;
        board = step(board, BoardEvent::Started { job, task: task(n) }).0;
        board = step(board, BoardEvent::NeedsSignIn(job)).0;
    }
    assert_eq!(state_of(&board, &coder), HelperState::Waiting(Need::SignIn));
}

#[test]
fn signing_in_opens_the_blocked_job_and_the_helper_takes_it() {
    let (board, coder) = create_app(Board::new(), "Coder");
    let (board, fx) = step(board, job_for(&coder, "tidy the repo", GoalOrigin::Person));
    let job = posted(&fx);
    let (board, _) = step(board, BoardEvent::Started { job, task: task(1) });
    let (board, _) = step(board, BoardEvent::NeedsSignIn(job));
    let (board, fx) = step(board, BoardEvent::SignedIn(coder.clone()));
    assert_eq!(launched(&fx), [(coder.clone(), job)]);
    assert_eq!(
        board.job(job).map(|j| j.status.clone()),
        Some(JobStatus::Taken(coder.clone()))
    );
    assert_ne!(state_of(&board, &coder), HelperState::Waiting(Need::SignIn));
}

#[test]
fn a_stored_card_and_its_new_kind_read_back() {
    let old = serde_json::to_string(&HelperKind::Native).expect("json");
    assert_eq!(old, r#""native""#);
    let app = HelperKind::AgentApp(prov::AgentProgram::parse("claude-code").expect("program"));
    let text = serde_json::to_string(&app).expect("json");
    assert_eq!(text, r#"{"agent_app":"claude-code"}"#);
    assert_eq!(
        serde_json::from_str::<HelperKind>(&text).expect("kind"),
        app
    );
    assert_eq!(
        serde_json::to_string(&Need::SignIn).expect("json"),
        r#""sign_in""#
    );
}
