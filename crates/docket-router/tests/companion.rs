//! The hosted `org.quire.Companion` actions: a worker is a child session under the spawner's
//! task policy, its goal is a handle, and a message to it is delivered as input.

mod support;

use docket_core::*;
use docket_router::{COMPANION_APP, Router, TaskState};
use prov::{
    Actor, AgentRef, AgentRole, EntityId, EntityKey, EntityKind, Labelled, MessageKind, SessionId,
};
use support::*;

fn companion_app() -> porter_core::AppName {
    app(COMPANION_APP)
}

fn args(pairs: &[(&str, Value)]) -> Args {
    pairs
        .iter()
        .map(|(name, value)| {
            (
                param(name),
                Labelled {
                    value: value.clone(),
                    label: trusted(),
                },
            )
        })
        .collect()
}

fn invocation(
    name: &str,
    session: &SessionId,
    target: TargetValue,
    arguments: Args,
    in_space: &str,
) -> Invocation {
    Invocation {
        call: CallId(1),
        action: prov::ActionName::parse(name).expect("action"),
        target,
        args: arguments,
        actor: Actor::Companion {
            session: session.clone(),
            role: AgentRole::Planner,
        },
        origin: Origin::Companion,
        space: space(in_space),
    }
}

fn start(goal: &str, session: &SessionId) -> Invocation {
    invocation(
        "companion.task.start",
        session,
        TargetValue::Nothing,
        args(&[
            ("goal", Value::Text(goal.into())),
            (
                "kind",
                Value::Choice(ChoiceId::parse("research").expect("choice")),
            ),
        ]),
        "work",
    )
}

fn task_entity(task: &str) -> EntityId {
    EntityId {
        app: companion_app(),
        kind: EntityKind::parse("companion.task").expect("kind"),
        key: EntityKey::parse(task).expect("key"),
    }
}

/// The task and the session `companion.task.start` answers: a record of both.
fn started(outcome: &Outcome) -> (prov::TaskId, SessionId) {
    let Some(Labelled {
        value: Value::Record(fields),
        ..
    }) = &outcome.value
    else {
        panic!("{outcome:?}")
    };
    let text = |name: &str| match fields.get(&param(name)) {
        Some(Value::Text(t)) => t.clone(),
        other => panic!("{name}: {other:?}"),
    };
    (
        prov::TaskId::parse(&text("task")).expect("a task id"),
        SessionId::parse(&text("session")).expect("a session id"),
    )
}

fn started_task(outcome: &Outcome) -> prov::TaskId {
    started(outcome).0
}

async fn worker_inbox(
    router: &Router<docket_fake::FakeSeams>,
    task: &prov::TaskId,
) -> Vec<InboundLine> {
    match ask(
        router,
        &companion(),
        IntentsRequest::MessageInbox(InboxAsk {
            agent: AgentRef::Worker { task: task.clone() },
            after: None,
        }),
    )
    .await
    {
        IntentsReply::Inbox(lines) => lines,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn starting_a_task_opens_a_child_session_holds_the_goal_as_a_handle_and_sends_it_as_a_request()
 {
    let router = router();
    let front = open(&router, "work", AgentRef::Companion).await;
    let parent_policy = wide_policy(&front.task, "work");
    give_policy(&router, &front.session, parent_policy.clone());

    let outcome = router
        .companion_perform(start("Find the Lisbon receipts", &front.session))
        .expect("the task starts");
    let (task, worker_session) = started(&outcome);
    assert_eq!(outcome.undo, Undoable::No);

    let (record, policy) = {
        let st = router.state.lock().expect("lock");
        let record = st
            .tasks
            .get(&task)
            .expect("the task is on the roster")
            .clone();
        let policy = st
            .sessions
            .get(&record.session)
            .expect("the child session")
            .policy
            .clone()
            .expect("a child of a task with a policy has one");
        (record, policy)
    };
    assert_eq!(
        record.session, worker_session,
        "the spawner is told the session the worker runs in"
    );
    assert_eq!(record.agent, AgentRef::Worker { task: task.clone() });
    assert_eq!(record.parent, Some(front.task.clone()));
    assert_eq!(record.state, TaskState::Working);
    assert!(
        matches!(record.goal, Reveal::Handle(_)),
        "the planner's goal is a handle, never plain text on the roster"
    );
    assert_eq!(
        compare(&policy, &parent_policy),
        PolicyChange::Same,
        "never wider than the parent's"
    );

    let records = router.seams.sink.records();
    assert!(
        records.iter().any(|r| matches!(
            r,
            AuditRecord::TaskStarted { task: t, by_call: CallId(1), parent: Some(p), .. }
                if *t == task && *p == front.task
        )),
        "{records:?}"
    );

    let inbox = worker_inbox(&router, &task).await;
    let [line] = inbox.as_slice() else {
        panic!("{inbox:?}")
    };
    assert_eq!(line.kind, MessageKind::Request);
    assert!(
        matches!(
            line.parts.as_slice(),
            [InboundPart::Text(Reveal::Handle(_))]
        ),
        "the worker reads the goal as a handle: {line:?}"
    );
}

#[tokio::test]
async fn a_child_of_a_task_with_no_policy_has_none() {
    let router = router();
    let front = open(&router, "work", AgentRef::Companion).await;
    let outcome = router
        .companion_perform(start("Watch the build", &front.session))
        .expect("starts");
    let task = started_task(&outcome);
    let st = router.state.lock().expect("lock");
    let session = &st.tasks.get(&task).expect("task").session;
    assert!(st.sessions.get(session).expect("session").policy.is_none());
}

#[tokio::test]
async fn only_a_companion_session_may_start_or_message_a_task() {
    let router = router();
    let front = open(&router, "work", AgentRef::Companion).await;
    let mut from_a_terminal = start("Do something", &front.session);
    from_a_terminal.actor = Actor::Cli;
    assert_eq!(
        router.companion_perform(from_a_terminal).map(|_| ()),
        Err(AppRefusal::Unsupported)
    );
    let mut anything_else = start("x", &front.session);
    anything_else.action = prov::ActionName::parse("companion.task.delete").expect("action");
    assert_eq!(
        router.companion_perform(anything_else).map(|_| ()),
        Err(AppRefusal::Unsupported)
    );
}

#[tokio::test]
async fn a_message_to_a_task_is_delivered_as_input_and_an_unknown_task_is_not_found() {
    let router = router();
    let front = open(&router, "work", AgentRef::Companion).await;
    let task = started_task(
        &router
            .companion_perform(start("Find receipts", &front.session))
            .expect("starts"),
    );
    let say_more = |target: &str, text: &str| {
        invocation(
            "companion.task.message",
            &front.session,
            TargetValue::Entities(vec![task_entity(target)]),
            args(&[("text", Value::Text(text.into()))]),
            "work",
        )
    };

    router
        .companion_perform(say_more(task.as_str(), "Also the Porto ones"))
        .expect("delivered");
    let inbox = worker_inbox(&router, &task).await;
    assert_eq!(inbox.len(), 2, "the goal, then this: {inbox:?}");
    assert_eq!(inbox[1].kind, MessageKind::Request);
    let InboundPart::Text(text) = &inbox[1].parts[0] else {
        panic!("{inbox:?}")
    };
    assert!(
        matches!(text, Reveal::Handle(_)),
        "words a planner typed are untrusted to the receiver: {text:?}"
    );

    assert_eq!(
        router
            .companion_perform(say_more("t-nobody", "hello"))
            .map(|_| ()),
        Err(AppRefusal::NotFound(task_entity("t-nobody")))
    );
    let st = router.state.lock().expect("lock");
    let worker = st
        .sessions
        .get(&st.tasks.get(&task).expect("task").session)
        .expect("session");
    assert_ne!(
        worker.saw.untrusted,
        Saw::NotSeen,
        "what the planner wrote taints the worker that reads it"
    );
}
