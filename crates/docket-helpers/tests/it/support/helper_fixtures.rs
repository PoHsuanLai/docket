//! What the helper tests share: the card, the reach, the jobs, the memory the router wrote, and
//! the editor's turn in a conversation.

use crate::support::world::*;
use almanac_core::{EventRef, EventSummary, JsonText, KindTag, RecentEntry, ReplicaId, Seq};
use companion_wire::{HelperForm, SayReply};
use docket_client::{InProcess, Intents};
use docket_core::helper::*;
use docket_core::{
    ActionMatch, AuditRecord, CallerRole, TaskPolicy, TaskPolicyState, TurnIn, TurnSource, TurnVia,
};
use docket_tasks::{HelperDesk, SayFromPerson};
use docket_vocab::{LabelText, Origin};
use porter_core::{AccountId, Count, ModelId};
use porter_infer::ModelRef;
use prov::{Actor, Effect, EntityKind, HelperId, UnixSeconds};
use serde_json::json;
use std::collections::BTreeSet;
use std::time::Duration;

/// Waits (the jobs run on tasks of their own) until `ready` holds.
pub async fn until(what: &str, mut ready: impl FnMut() -> bool) {
    for _ in 0..500 {
        if ready() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("never happened: {what}");
}

pub fn state_of(hub: &TestHub, helper: &HelperId) -> HelperState {
    hub.board().card(helper).expect("card").state.clone()
}

pub fn status_of(hub: &TestHub, job: JobId) -> JobStatus {
    hub.board().job(job).expect("job").status.clone()
}

/// What a helper may reach in the tests: Mail's threads, up to undoable writes.
pub fn reach() -> TaskPolicy {
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

pub fn model() -> HelperModel {
    HelperModel::Named(ModelRef {
        account: AccountId::parse("local").expect("account"),
        model: ModelId::parse("tiny").expect("model"),
    })
}

pub fn draft(name: &str, model: HelperModel) -> CardDraft {
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

pub fn goal(text: &str) -> GoalText {
    GoalText::parse(text).expect("goal")
}

pub fn created(effects: &[BoardEffect]) -> HelperId {
    effects
        .iter()
        .find_map(|e| match e {
            BoardEffect::Created(id) => Some(id.clone()),
            _ => None,
        })
        .expect("a card was made")
}

pub fn person_job(to: &HelperId, text: &str) -> BoardEvent {
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
pub fn remembered(w: &World) -> Vec<RecentEntry> {
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

pub fn posted(effects: &[BoardEffect]) -> JobId {
    effects
        .iter()
        .find_map(|e| match e {
            BoardEffect::Posted(id) => Some(*id),
            _ => None,
        })
        .expect("a job was made")
}

pub fn chosen_ref() -> ModelRef {
    ModelRef {
        account: AccountId::parse("local").expect("account"),
        model: ModelId::parse("tiny").expect("model"),
    }
}

pub const READ: &str = "org_quire_Mail-mail_thread_read";
pub const ARCHIVE: &str = "org_quire_Mail-mail_thread_archive";
pub const DELETE: &str = "org_quire_Mail-mail_thread_delete";

pub fn thread(key: &str) -> serde_json::Value {
    json!({ "target": { "app": "org.quire.Mail", "kind": "mail.thread", "key": key } })
}

/// A target list of one thread, for the actions that act on many.
pub fn threads(key: &str) -> serde_json::Value {
    json!({ "target": [{ "app": "org.quire.Mail", "kind": "mail.thread", "key": key }] })
}

pub fn work_form(name: &str, model: HelperModel) -> HelperForm {
    HelperForm {
        name: HelperName::parse(name).expect("name"),
        role: Role::parse("Looks after the mail.").expect("role"),
        space: space("work"),
        model,
        apps: vec![app("org.quire.Mail")],
        ceiling: Ceiling::Change,
        steps: Count(8),
        kind: Default::default(),
    }
}

/// The editor records the person's words in `session` with the router, as its own surface, and
/// asks the hub to run the turn.
pub async fn editor_says(
    w: &World,
    hub: &TestHub,
    session: &prov::SessionId,
    text: &str,
) -> SayReply {
    let editor = Intents::over(InProcess::new(
        w.router.clone(),
        caller("org.quire.Acp", CallerRole::Editor),
    ));
    let id = editor
        .session_turn(
            session.clone(),
            TurnIn {
                text: text.into(),
                origin: Origin::InWindowField,
                keep: keep_nothing(),
                via: TurnVia::Typed,
                mode: None,
            },
        )
        .await
        .expect("the router records the editor's turn in a conversation");
    // By the trait: the hub's own `say` tells the person a sentence about a helper.
    HelperDesk::say(
        hub,
        SayFromPerson {
            session: session.clone(),
            id,
            text: text.into(),
            from: TurnSource::Editor(app("org.quire.Acp")),
        },
    )
    .await
}
