//! Many tasks in one app: two tasks and the front pointer between them, a side conversation that
//! ends as an episode, and restart recovery from what memory holds. The model is scripted, the
//! memory is almanac-fake's backend, the clock is virtual.

mod support;

use almanac_client::InProcess;
use almanac_core::{BodyMode, Caller, MemoryReply, MemoryRequest, RecentQuery, TrustFilter};
use almanac_fake::{ScriptedConsolidator, fake_service};
use docket_core::RosterState;
use docket_inapp::{AlmanacMemory, AuditTo, Ending, InAppAgent, InAppKit};
use porter_core::Count;
use prov::{AgentRef, TaskId, UnixSeconds};
use std::sync::Arc;
use support::TestSheet;
use support::host::{clock, parts};
use support::infer::{ScriptedInfer, call, words};

fn ask_which() -> support::infer::Say {
    call(
        "quire_ask",
        serde_json::json!({ "text": "Which one?", "choices": ["a", "b"] }),
    )
}

fn worker(task: &str) -> AgentRef {
    AgentRef::Worker {
        task: TaskId::parse(task).expect("task"),
    }
}

#[tokio::test]
async fn two_tasks_keep_their_own_words_and_the_front_pointer_follows_the_person() {
    let model = ScriptedInfer::new(vec![
        ask_which(),
        words("Sunny."),
        words("Done with the tidy."),
    ]);
    let mut agent = InAppAgent::new(parts(&model, &TestSheet::default(), &clock())).expect("agent");

    let tidy = agent.new_task().await.expect("task");
    assert_eq!(agent.front().task.as_ref(), Some(&tidy));
    let first = agent.ask_in(&tidy, "tidy the inbox").await.expect("turn");
    assert!(matches!(first.ending, Ending::Asked { .. }), "{first:?}");

    // A second task opens while the first waits for its answer: the front pointer moves.
    let weather = agent.new_task().await.expect("task");
    assert_ne!(weather, tidy);
    assert_eq!(agent.front().task.as_ref(), Some(&weather));
    let second = agent
        .ask_in(&weather, "what is the weather")
        .await
        .expect("turn");
    assert_eq!(second.ending, Ending::Done);
    assert_eq!(second.task, weather);
    assert_eq!(
        agent.front().task,
        None,
        "the front task ended, so the launcher has nothing to return to"
    );
    assert_eq!(
        agent.open_tasks(),
        vec![tidy.clone()],
        "the finished task was dismissed"
    );

    // Answering the first task moves the pointer back, and it still has its own words.
    let third = agent.ask_in(&tidy, "a").await.expect("turn");
    assert_eq!(third.ending, Ending::Done);
    let view = model.user_text(2);
    assert!(view.contains("tidy the inbox"), "{view}");
    assert!(
        view.contains("You said: tidy the inbox\nYou said: a"),
        "{view}"
    );
    assert!(
        !view.contains("You said: what is the weather"),
        "tasks do not share turns (the other one is on the roster): {view}"
    );
    assert_eq!(agent.phase_of(&tidy), None, "and it too is over");
}

#[tokio::test]
async fn the_roster_shows_the_task_that_waits_and_remembers_the_one_that_ended() {
    let model = ScriptedInfer::new(vec![ask_which(), words("Sunny.")]);
    let mut agent = InAppAgent::new(parts(&model, &TestSheet::default(), &clock())).expect("agent");
    let tidy = agent.new_task().await.expect("task");
    agent.ask_in(&tidy, "tidy the inbox").await.expect("turn");
    let weather = agent.new_task().await.expect("task");
    agent
        .ask_in(&weather, "what is the weather")
        .await
        .expect("turn");

    let states: Vec<RosterState> = agent.roster().entries.iter().map(|l| l.state).collect();
    assert!(states.contains(&RosterState::NeedsYou), "{states:?}");
    assert!(states.contains(&RosterState::Done), "{states:?}");
}

#[tokio::test]
async fn a_side_conversation_ends_as_an_episode_of_the_persons_own_words() {
    let service = Arc::new(fake_service(ScriptedConsolidator::default()));
    let model = ScriptedInfer::new(vec![ask_which()]);
    let clock = clock();
    let mut agent = InAppAgent::with_kit(
        parts(&model, &TestSheet::default(), &clock),
        InAppKit::default()
            .memory(AlmanacMemory::over(InProcess::new(
                service.clone(),
                Caller::Router,
            )))
            .audit_to(AuditTo::Memory),
    )
    .expect("agent");
    // A task of the companion is open in the Space; the episode goes to the router through it.
    let front = agent.new_task().await.expect("task");
    agent.ask_in(&front, "tidy the inbox").await.expect("turn");

    agent.told(worker("w-9"), "go through accounting, nothing else");
    assert_eq!(agent.side_conversations(), 1);
    let reader = AlmanacMemory::over(InProcess::new(service.clone(), Caller::Router));
    let before = side_episodes(&reader).await;

    clock.advance(119);
    agent.tick().await.expect("tick");
    assert_eq!(agent.side_conversations(), 1, "still within the quiet time");
    clock.advance(2);
    agent.tick().await.expect("tick");
    assert_eq!(agent.side_conversations(), 0);
    assert_eq!(side_episodes(&reader).await, before + 1);
}

/// How many episode records memory holds.
async fn side_episodes(reader: &impl docket_router::MemoryLink) -> usize {
    let reply = docket_router::MemoryLink::ask(
        reader,
        MemoryRequest::Recent(
            support::host::work(),
            RecentQuery {
                since: UnixSeconds(0),
                kinds: Vec::new(),
                trust: TrustFilter::Any,
                limit: Count(200),
                bodies: BodyMode::Json,
            },
        ),
    )
    .await
    .expect("recent");
    let MemoryReply::Recent(entries) = reply else {
        panic!("recent: {reply:?}");
    };
    entries
        .iter()
        .filter(|e| format!("{:?}", e.summary.kind).contains("episode"))
        .count()
}

#[tokio::test]
async fn a_restart_picks_up_the_task_that_was_unfinished_from_what_memory_holds() {
    let service = Arc::new(fake_service(ScriptedConsolidator::default()));
    let kit = || {
        InAppKit::default()
            .memory(AlmanacMemory::over(InProcess::new(
                service.clone(),
                Caller::Router,
            )))
            .audit_to(AuditTo::Memory)
    };

    let model = ScriptedInfer::new(vec![ask_which()]);
    let mut before =
        InAppAgent::with_kit(parts(&model, &TestSheet::default(), &clock()), kit()).expect("agent");
    let task = before.new_task().await.expect("task");
    let asked = before.ask_in(&task, "plan the trip").await.expect("turn");
    assert!(matches!(asked.ending, Ending::Asked { .. }), "{asked:?}");
    drop(before);

    // The next run knows nothing but memory: it finds the front task and the agent that waited.
    let model = ScriptedInfer::new(vec![words("Welcome back.")]);
    let mut after =
        InAppAgent::with_kit(parts(&model, &TestSheet::default(), &clock()), kit()).expect("agent");
    assert_eq!(after.front().task, None);
    let resumed = after
        .restore()
        .await
        .expect("restore")
        .expect("an unfinished task");
    assert_eq!(after.front().task.as_ref(), Some(&resumed));
    let states: Vec<RosterState> = after.roster().entries.iter().map(|l| l.state).collect();
    assert!(
        states.iter().any(|s| *s != RosterState::Done),
        "the agent that was working is on the roster again: {states:?}"
    );
    let reply = after.ask("where were we").await.expect("turn");
    assert_eq!(
        reply.task, resumed,
        "the person goes on in the reopened task"
    );
    assert_eq!(reply.ending, Ending::Done);
}

#[tokio::test]
async fn a_restart_after_everything_finished_has_nothing_to_pick_up() {
    let service = Arc::new(fake_service(ScriptedConsolidator::default()));
    let kit = || {
        InAppKit::default()
            .memory(AlmanacMemory::over(InProcess::new(
                service.clone(),
                Caller::Router,
            )))
            .audit_to(AuditTo::Memory)
    };
    let model = ScriptedInfer::new(vec![words("Hello.")]);
    let mut before =
        InAppAgent::with_kit(parts(&model, &TestSheet::default(), &clock()), kit()).expect("agent");
    assert_eq!(before.ask("hi").await.expect("turn").ending, Ending::Done);
    drop(before);

    let mut after = InAppAgent::with_kit(
        parts(&ScriptedInfer::new(vec![]), &TestSheet::default(), &clock()),
        kit(),
    )
    .expect("agent");
    assert_eq!(after.restore().await.expect("restore"), None);
}
