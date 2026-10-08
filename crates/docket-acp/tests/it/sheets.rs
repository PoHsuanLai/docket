//! The router's sheet in the editor: `allow_always` appears only when the sheet offered it, says
//! what it covers in words built from the typed scope, and a click on it reaches the host as the
//! "always" choice. Once the person has said it for an action, the editor's own extra prompt
//! stands down for that action while the router's sheet still decides what the grant covers.

use crate::support::*;
use docket_acp::always_words;
use docket_core::{
    AbsPath, AlwaysOffer, Anchor, CommandPrefix, ConfirmDetail, ConfirmId, ConfirmOffer,
    ConfirmParts, ConfirmRequest, Domain, Gesture, LabelText, Recipient, Seconds, StandingScope,
    TaintNote, Withheld,
};
use docket_session::fake::MemoryLog;
use docket_session::{BackendEvent, SheetChoice, TurnEnd};
use porter_core::Count;
use prov::{Actor, Effect, SpaceId};
use serde_json::{Value, json};
use std::sync::Arc;

const EDITOR: &str = "acp.zed";

fn files_scope(under: &str) -> StandingScope {
    StandingScope::Files {
        action: action("mail.archive"),
        under: AbsPath::parse(under).expect("path"),
    }
}

fn sheet_with(id: &str, always: AlwaysOffer) -> ConfirmRequest {
    ConfirmRequest::new(ConfirmParts {
        id: ConfirmId::parse(id).expect("id"),
        space: SpaceId::desktop(),
        actor: Actor::Unknown,
        app: app("org.quire.Mail"),
        action: LabelText::parse("Archive messages").expect("label"),
        effect: Effect::UndoableWrite,
        count: Count(1),
        detail: ConfirmDetail::Plain,
        lines: Vec::new(),
        why: Vec::new(),
        taint: TaintNote::Clean,
        offer: ConfirmOffer::OnceOnly,
        gesture: Gesture::Press,
        anchor: Anchor::Centre,
        expires: Seconds(120),
    })
    .offering(always)
}

fn sheet(id: &str, always: AlwaysOffer) -> BackendEvent {
    BackendEvent::Sheet(Box::new(sheet_with(id, always)))
}

fn offered() -> AlwaysOffer {
    AlwaysOffer::Offered(files_scope("/home/u/docs"))
}

fn withheld() -> AlwaysOffer {
    AlwaysOffer::Withheld(Withheld::UntrustedIntoSink)
}

fn pick(option: &'static str) -> impl FnMut(&Value) -> Option<Value> {
    move |_| Some(json!({"outcome": {"outcome": "selected", "optionId": option}}))
}

fn kinds(request: &Value) -> Vec<String> {
    request["params"]["options"]
        .as_array()
        .expect("options")
        .iter()
        .map(|o| o["kind"].as_str().expect("kind").to_owned())
        .collect()
}

/// One turn that puts `sheets` to the editor in order.
fn script(sheets: Vec<BackendEvent>) -> Vec<Vec<Vec<BackendEvent>>> {
    let mut events = sheets;
    events.push(end(TurnEnd::Done));
    vec![vec![events]]
}

#[tokio::test]
async fn an_offered_scope_shows_allow_always_in_words_and_a_click_reaches_the_host() {
    let log = Arc::new(MemoryLog::new());
    let (mut server, mut ed) = rig(&log, script(vec![sheet("c-1", offered())]), EDITOR);
    let script = async move {
        ed.initialize().await;
        let session = ed.new_session("/work").await;
        ed.prompt(&session, "archive", &mut pick("allow_always"))
            .await;
        let asked = ed.permission_requests();
        assert_eq!(asked.len(), 1);
        assert_eq!(
            kinds(&asked[0]),
            ["allow_once", "allow_always", "reject_once"]
        );
        let always = &asked[0]["params"]["options"][1];
        assert_eq!(always["optionId"], "allow_always");
        assert_eq!(
            always["name"],
            "Always allow \"Archive messages\" on files under /home/u/docs"
        );
    };
    let (ran, ()) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
    let id = ConfirmId::parse("c-1").expect("id");
    assert_eq!(server.into_host().sheets, vec![(id, SheetChoice::Always)]);
}

#[tokio::test]
async fn a_withheld_offer_shows_no_allow_always_and_choosing_it_anyway_is_a_refusal() {
    let log = Arc::new(MemoryLog::new());
    let (mut server, mut ed) = rig(&log, script(vec![sheet("c-1", withheld())]), EDITOR);
    let script = async move {
        ed.initialize().await;
        let session = ed.new_session("/work").await;
        ed.prompt(&session, "send", &mut pick("allow_always")).await;
        let asked = ed.permission_requests();
        assert_eq!(kinds(&asked[0]), ["allow_once", "reject_once"]);
        assert!(
            !ed.transcript().contains("allow_always"),
            "allow_always never appears when withheld"
        );
    };
    let (ran, ()) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
    let host = server.into_host();
    assert_eq!(host.sheets.len(), 1);
    assert_eq!(host.sheets[0].1, SheetChoice::Refused);
}

#[tokio::test]
async fn once_and_reject_reach_the_host_as_themselves() {
    let log = Arc::new(MemoryLog::new());
    let two = script(vec![sheet("c-1", offered()), sheet("c-2", offered())]);
    let (mut server, mut ed) = rig(&log, two, EDITOR);
    let script = async move {
        ed.initialize().await;
        let session = ed.new_session("/work").await;
        let mut n = 0;
        let mut answer = move |_: &Value| {
            n += 1;
            let option = if n == 1 { "allow_once" } else { "reject_once" };
            Some(json!({"outcome": {"outcome": "selected", "optionId": option}}))
        };
        ed.prompt(&session, "go", &mut answer).await;
    };
    let (ran, ()) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
    let chosen: Vec<SheetChoice> = server.into_host().sheets.iter().map(|s| s.1).collect();
    assert_eq!(chosen, [SheetChoice::Once, SheetChoice::Refused]);
}

#[tokio::test]
async fn a_cancel_or_an_error_answers_no_sheet_as_a_yes() {
    let log = Arc::new(MemoryLog::new());
    let (mut server, mut ed) = rig(&log, script(vec![sheet("c-1", offered())]), EDITOR);
    let script = async move {
        ed.initialize().await;
        let session = ed.new_session("/work").await;
        let mut cancelled = |_: &Value| Some(json!({"outcome": {"outcome": "cancelled"}}));
        ed.prompt(&session, "go", &mut cancelled).await;
    };
    let (ran, ()) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
    assert_eq!(server.into_host().sheets[0].1, SheetChoice::Refused);
}

#[tokio::test]
async fn after_always_the_editors_own_prompt_stands_down_for_that_action_only() {
    let log = Arc::new(MemoryLog::new());
    let turn = |first: bool| {
        let mut events = Vec::new();
        if first {
            events.push(started(1, "mail.archive", Effect::UndoableWrite));
            events.push(sheet("c-1", offered()));
        } else {
            events.push(started(2, "mail.archive", Effect::UndoableWrite));
            events.push(started(3, "mail.forward", Effect::Outbound));
        }
        events.push(end(TurnEnd::Done));
        events
    };
    let (mut server, mut ed) = rig(&log, vec![vec![turn(true), turn(false)]], EDITOR);
    let script = async move {
        ed.initialize().await;
        let session = ed.new_session("/work").await;
        // First turn: the gate at the start asks (allow once), then the sheet (allow always).
        let mut n = 0;
        let mut answer = move |_: &Value| {
            n += 1;
            let option = if n == 1 { "allow_once" } else { "allow_always" };
            Some(json!({"outcome": {"outcome": "selected", "optionId": option}}))
        };
        ed.prompt(&session, "archive", &mut answer).await;
        assert_eq!(ed.permission_requests().len(), 2);
        // Second turn: the same action asks nothing; another action still asks.
        ed.prompt(&session, "again", &mut pick("allow_once")).await;
        let asked = ed.permission_requests();
        assert_eq!(asked.len(), 3, "only the forward asked");
        assert_eq!(asked[2]["params"]["toolCall"]["title"], "mail.forward");
    };
    let (ran, ()) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
}

#[tokio::test]
async fn the_words_come_from_the_typed_scope_for_each_kind() {
    let terminal = StandingScope::Terminal {
        action: action("term.run"),
        command: CommandPrefix::parse("cargo test").expect("prefix"),
        cwd: AbsPath::parse("/work/proj").expect("cwd"),
    };
    let to_address = StandingScope::Outbound {
        action: action("mail.send"),
        to: Recipient::address("a@example.test").expect("address"),
    };
    let to_domain = StandingScope::Outbound {
        action: action("mail.send"),
        to: Recipient::Domain(Domain::parse("example.test").expect("domain")),
    };
    assert_eq!(
        always_words("Run", &terminal),
        "Always allow \"Run\" for commands starting \"cargo test\" in /work/proj and below"
    );
    assert_eq!(
        always_words("Send", &to_address),
        "Always allow \"Send\" to a@example.test"
    );
    assert_eq!(
        always_words("Send", &to_domain),
        "Always allow \"Send\" to anyone at example.test"
    );
}
