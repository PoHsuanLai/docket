//! Completion notes reach only a live front task; restart rebuilds from a source.

use agent_loop::*;
use almanac_core::{
    BodyMode, EventRef, EventSummary, JsonText, KindTag, RecentEntry, RecentQuery, ReplicaId, Seq,
};
use companion_wire::{AnswerPhase, NeedsYou, SessionRecord};
use companiond::*;
use docket_core::RosterState;
use prov::{Actor, AgentRef, ReportStatus, RunId, SpaceId, TaskId};
use std::sync::Mutex;

fn note(status: ReportStatus, attention: Attention) -> CompletionNote {
    CompletionNote {
        agent: AgentRef::Cua {
            run: RunId::parse("r-3").expect("run"),
        },
        status,
        steps: porter_count(9),
        values: porter_count(2),
        attention,
    }
}

fn working() -> LoopState {
    LoopState {
        phase: LoopPhase::Planning,
        turn: None,
        steps: 1,
        pending: vec![],
    }
}

#[test]
fn a_quiet_completion_is_one_line_in_a_live_front_task() {
    let effects = completion_effects(
        &note(ReportStatus::Done, Attention::Quiet),
        Some(&working()),
    );
    assert_eq!(
        effects,
        [LoopEffect::Note(
            "run r-3 finished: Done, 9 steps, 2 values".into()
        )]
    );
}

#[test]
fn a_result_that_needs_the_person_also_makes_the_answer_wait_for_them() {
    let effects = completion_effects(
        &note(ReportStatus::Failed, Attention::NeedsYou),
        Some(&working()),
    );
    assert_eq!(effects.len(), 2);
    assert!(matches!(
        &effects[1],
        LoopEffect::Publish(AnswerPhase::NeedsYou(NeedsYou::Question { .. }))
    ));
}

#[test]
fn nothing_is_added_to_a_finished_or_absent_front_task_or_for_progress() {
    let finished = LoopState {
        phase: LoopPhase::Finished(FinishedAs::Done),
        turn: None,
        steps: 3,
        pending: vec![],
    };
    assert!(
        completion_effects(&note(ReportStatus::Done, Attention::Quiet), Some(&finished)).is_empty()
    );
    assert!(completion_effects(&note(ReportStatus::Done, Attention::Quiet), None).is_empty());
    assert!(
        completion_effects(
            &note(ReportStatus::Progress, Attention::Quiet),
            Some(&working())
        )
        .is_empty()
    );
}

struct Scripted(
    Result<Vec<RecentEntry>, ReplayFault>,
    Mutex<Vec<RecentQuery>>,
);

impl Scripted {
    fn new(entries: Result<Vec<RecentEntry>, ReplayFault>) -> Self {
        Self(entries, Mutex::new(vec![]))
    }
}

impl RecentSource for Scripted {
    async fn recent(&self, query: RecentQuery) -> Result<Vec<RecentEntry>, ReplayFault> {
        self.1.lock().expect("log").push(query);
        self.0.clone()
    }
}

fn entry(seq: u64, kind: &str, at: i64, body: Option<String>) -> RecentEntry {
    RecentEntry {
        summary: EventSummary {
            event: EventRef {
                space: SpaceId::parse("work").expect("space"),
                replica: ReplicaId([1; 16]),
                seq: Seq(seq),
            },
            occurred: prov::UnixSeconds(at),
            kind: KindTag::parse(kind).expect("kind"),
            actor: Actor::User {
                via: porter_core::AppName::parse("org.quire.Shell").expect("app"),
            },
            things: vec![],
        },
        effect: prov::Effect::Read,
        label: prov::Label::trusted_user(),
        text: None,
        body: body.map(|b| JsonText::parse(&b).expect("json")),
    }
}

fn opened(task: &TaskId) -> String {
    serde_json::to_string(&SessionRecord::Opened {
        task: task.clone(),
        space: SpaceId::parse("work").expect("space"),
        agent: AgentRef::Companion,
        parent: None,
    })
    .expect("json")
}

#[tokio::test]
async fn restart_rebuilds_the_front_and_the_roster_from_recent_with_bodies() {
    let task = TaskId::parse("t-1").expect("task");
    let source = Scripted::new(Ok(vec![
        // Newest first, as `Recent` answers; a kind the rebuild does not read is skipped, and so
        // is an event whose body is gone.
        entry(3, "docket.call", 3, Some("{}".into())),
        entry(2, "companion.session.closed", 2, None),
        entry(1, "companion.session.opened", 1, Some(opened(&task))),
    ]));
    let rebuilt = recover(&source, prov::UnixSeconds(0))
        .await
        .expect("rebuilt");
    assert_eq!(rebuilt.front, Some(task));
    assert_eq!(rebuilt.roster().entries[0].state, RosterState::Working);
    let queries = source.1.lock().expect("log");
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].bodies, BodyMode::Json);
    assert_eq!(queries[0].since, prov::UnixSeconds(0));
    let kinds: Vec<&str> = queries[0].kinds.iter().map(|k| k.as_str()).collect();
    assert_eq!(
        kinds,
        vec![
            "companion.session.*",
            "companion.message",
            "companion.episode"
        ]
    );
}

#[test]
fn replay_reads_oldest_first_and_refuses_a_body_it_cannot_parse() {
    let task = TaskId::parse("t-1").expect("task");
    let two = vec![
        entry(2, "companion.session.opened", 20, Some(opened(&task))),
        entry(1, "companion.session.opened", 10, Some(opened(&task))),
    ];
    let events = replay_of(&two).expect("events");
    assert_eq!(
        events.iter().map(|e| e.at.0).collect::<Vec<_>>(),
        vec![10, 20]
    );
    for kind in [
        "companion.session.opened",
        "companion.message",
        "companion.episode",
    ] {
        let bad = vec![entry(1, kind, 1, Some("{\"nope\":1}".into()))];
        assert_eq!(replay_of(&bad), Err(ReplayFault::Malformed), "{kind}");
    }
}

#[tokio::test]
async fn an_unreadable_record_is_a_fault_not_a_guess() {
    let since = prov::UnixSeconds(0);
    assert_eq!(
        recover(&Scripted::new(Err(ReplayFault::Malformed)), since).await,
        Err(ReplayFault::Malformed)
    );
    assert_eq!(
        recover(&Scripted::new(Err(ReplayFault::Unavailable)), since).await,
        Err(ReplayFault::Unavailable)
    );
}

#[test]
fn the_binary_is_a_skeleton_that_exits_two() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_companiond"))
        .output()
        .expect("runs");
    assert_eq!(output.status.code(), Some(2));
}

fn porter_count(n: u32) -> porter_core::Count {
    porter_core::Count(n)
}
