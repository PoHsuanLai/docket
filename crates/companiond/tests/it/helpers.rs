//! Helpers: the board companiond owns, its records, and the jobs it runs. Everything here runs
//! over docket-fake's router in process, a scripted model per helper, and memory the test fills.

use crate::support::infer::ScriptedInfer;
use crate::support::world::*;
use almanac_core::{
    EventRef, EventSummary, JsonText, KindTag, MemoryReply, RecentEntry, ReplicaId, Seq,
};
use companiond::Hub;
use docket_client::InProcess;
use docket_core::helper::*;
use docket_core::{ActionMatch, AuditRecord, LabelText, TaskPolicy, TaskPolicyState};
use docket_fake::FakeSeams;
use porter_core::{AccountId, Count, ModelId};
use porter_infer::choice::ModelRef;
use prov::{Actor, Effect, EntityKind, HelperId, UnixSeconds};
use std::collections::BTreeSet;

type TestHub = Hub<ScriptedInfer, InProcess<FakeSeams>>;

fn hub_of(w: &World) -> TestHub {
    Hub::new(w.restarted(), w.companion.shared.clone())
}

/// What a helper may reach in the tests: Mail's threads, up to undoable writes.
fn reach() -> TaskPolicy {
    TaskPolicy {
        task: task("t-0"),
        space: space("work"),
        from: vec![],
        actions: BTreeSet::from([ActionMatch::AppUpTo(
            app("org.quire.Mail"),
            Effect::UndoableWrite,
        )]),
        kinds: BTreeSet::from([EntityKind::parse("mail.thread").expect("kind")]),
        ceiling: Effect::UndoableWrite,
        max_count: Count(10),
        recipients: vec![],
        destinations: vec![],
        paths: vec![],
        expires: UnixSeconds(i64::MAX),
        rationale: LabelText::parse("what the helper may use").expect("words"),
        state: TaskPolicyState::Active,
    }
}

fn model() -> HelperModel {
    HelperModel::Named(ModelRef {
        account: AccountId::parse("local").expect("account"),
        model: ModelId::parse("tiny").expect("model"),
    })
}

fn draft(name: &str, model: HelperModel) -> CardDraft {
    CardDraft {
        name: HelperName::parse(name).expect("name"),
        role: Role::parse("Looks after the mail.").expect("role"),
        kind: HelperKind::Native,
        space: space("work"),
        model,
        reach: reach(),
        budget: HelperBudget { steps: Count(8) },
    }
}

fn goal(text: &str) -> GoalText {
    GoalText::parse(text).expect("goal")
}

fn created(effects: &[BoardEffect]) -> HelperId {
    effects
        .iter()
        .find_map(|e| match e {
            BoardEffect::Created(id) => Some(id.clone()),
            _ => None,
        })
        .expect("a card was made")
}

fn person_job(to: &HelperId, text: &str) -> BoardEvent {
    BoardEvent::Post(Box::new(NewJob {
        to: to.clone(),
        goal: goal(text),
        origin: GoalOrigin::Person,
        giver: None,
        owns: vec![],
        with: vec![],
    }))
}

/// The helper records the router was handed, as memory would answer them: newest first.
fn remembered(w: &World) -> Vec<RecentEntry> {
    let mut entries: Vec<RecentEntry> = w
        .records()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Helper { at, kind, json, .. } => Some((at, kind, json)),
            _ => None,
        })
        .enumerate()
        .map(|(n, (at, kind, json))| RecentEntry {
            summary: EventSummary {
                event: EventRef {
                    space: space("work"),
                    replica: ReplicaId([1; 16]),
                    seq: Seq(n as u64 + 1),
                },
                occurred: at,
                kind: KindTag::parse(kind.slug()).expect("kind"),
                actor: Actor::System {
                    part: prov::SystemPart::Router,
                },
                things: vec![],
            },
            effect: Effect::Read,
            label: prov::Label::trusted_user(),
            text: None,
            body: JsonText::parse(json.as_str()).ok(),
        })
        .collect();
    entries.reverse();
    entries
}

#[tokio::test]
async fn the_board_is_written_through_the_router_and_comes_back_after_a_restart() {
    let w = world(vec![]);
    let hub = hub_of(&w);
    // One helper with no model and a job waiting for it, and one with a model and nothing to do.
    let research = created(
        &hub.apply(BoardEvent::Create(Box::new(draft(
            "Research",
            HelperModel::Unset,
        ))))
        .await,
    );
    let inbox = created(
        &hub.apply(BoardEvent::Create(Box::new(draft("Inbox", model()))))
            .await,
    );
    hub.apply(person_job(&research, "compare the three offers"))
        .await;
    let board = hub.board();
    assert_eq!(board.cards().len(), 2);
    assert_eq!(
        board.card(&research).map(|c| c.state.clone()),
        Some(HelperState::Waiting(Need::ChooseModel))
    );

    // The router was handed a card per helper and a job, each under its own kind.
    let kinds: Vec<HelperRecordKind> = w
        .records()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Helper { kind, .. } => Some(kind),
            _ => None,
        })
        .collect();
    assert!(kinds.contains(&HelperRecordKind::Card));
    assert!(kinds.contains(&HelperRecordKind::Job));
    assert!(!kinds.contains(&HelperRecordKind::Post));

    // The person was told once that Research needs a model.
    let told = hub.told_since(0);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].text.contains("Research needs a model"),
        "{}",
        told[0].text
    );

    // A restart: a new companion over a new router whose memory holds those records.
    let again = world_with(
        vec![],
        vec![MemoryReply::Ok, MemoryReply::Recent(remembered(&w))],
    );
    let back = hub_of(&again);
    back.restore(&[space("work")]).await;
    let board = back.board();
    let names: Vec<&str> = board.cards().iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Research", "Inbox"]);
    assert_eq!(board.jobs().len(), 1);
    assert_eq!(board.jobs()[0].goal.as_str(), "compare the three offers");
    assert_eq!(board.jobs()[0].source, GoalSource::Person);
    // Nothing was running, and the helper without a model still waits for one.
    assert_eq!(
        board.card(&research).map(|c| c.state.clone()),
        Some(HelperState::Waiting(Need::ChooseModel))
    );
    assert_eq!(
        board.card(&inbox).map(|c| c.state.clone()),
        Some(HelperState::Idle)
    );
    assert_eq!(
        board.cards()[1].model,
        model(),
        "the model the person chose is the card's own"
    );
}
