//! The editor's face of the helpers: `session/list` shows the helpers of the workspace's Space,
//! `session/new` with `_meta.helper` opens a job, a prompt is the person's turn with the editor's
//! caps, the board's sentences arrive as plain lines, and "always" reaches the host as chosen.
//! A scripted editor over the existing transport; the helpers are a fake seam.

use crate::support::*;
use bulkhead::AbsPath;
use companion_model::helper::{HelperName, JobId};
use companion_wire::{JobLine, JobShown, ToldWire};
use docket_acp::{ConversationTurns, HelperRefusal, HelperRow, Helpers};
use docket_core::{
    Anchor, AppRefusal, CallRefusal, ConfirmDetail, ConfirmId, ConfirmOffer, ConfirmParts,
    ConfirmRequest, FailText, Gesture, StepEnd, TaintNote, TurnSource, UserTurn,
};
use docket_permissions_core::{AlwaysOffer, StandingScope};
use docket_session::fake::MemoryLog;
use docket_session::{BackendEvent, BackendKind, Opening, Seq, SessionEntry, SessionLog};
use docket_session::{SheetChoice, TurnEnd};
use docket_vocab::{LabelText, Seconds};
use porter_ids::Count;
use prov::{Actor, AgentRef, Effect, HelperId, SessionId, SpaceId, TaskId};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

const EDITOR: &str = "acp.zed";

fn helper(n: u64) -> HelperId {
    serde_json::from_str(&format!("\"h-{n}\"")).expect("helper id")
}

fn session(n: &str) -> SessionId {
    SessionId::parse(n).expect("session")
}

fn row(n: u64, name: &str, space: &str, session_id: &str) -> HelperRow {
    HelperRow {
        id: helper(n),
        name: HelperName::parse(name).expect("name"),
        space: match space {
            "desktop" => SpaceId::desktop(),
            other => SpaceId::parse(other).expect("space"),
        },
        session: session(session_id),
    }
}

/// What the fake seam was asked, for the test to read after the server ran.
#[derive(Debug, Default)]
struct Seen {
    opened: Vec<HelperId>,
    /// The turns taken to a helper's conversation, with the session they were said in.
    said: Vec<(SessionId, UserTurn)>,
}

struct FakeHelpers {
    rows: Vec<HelperRow>,
    jobs: Vec<(HelperId, Vec<JobLine>)>,
    job_session: SessionId,
    told: VecDeque<ToldWire>,
    seen: Arc<Mutex<Seen>>,
    /// What the helper answers in its conversation: its words, or why it could not.
    answer: Result<String, String>,
}

impl FakeHelpers {
    fn new(rows: Vec<HelperRow>) -> (Self, Arc<Mutex<Seen>>) {
        let seen = Arc::new(Mutex::new(Seen::default()));
        let fake = FakeHelpers {
            rows,
            jobs: Vec::new(),
            job_session: session("s-70"),
            told: VecDeque::new(),
            seen: seen.clone(),
            answer: Ok("Offer B is the best.".to_owned()),
        };
        (fake, seen)
    }
}

impl ConversationTurns for FakeHelpers {
    async fn say(&mut self, session: &SessionId, turn: UserTurn) -> Result<String, HelperRefusal> {
        self.seen
            .lock()
            .expect("seen")
            .said
            .push((session.clone(), turn));
        self.answer.clone().map_err(HelperRefusal)
    }
}

impl Helpers for FakeHelpers {
    fn rows(&self) -> Vec<HelperRow> {
        self.rows.clone()
    }

    fn jobs(&self, helper: &HelperId) -> Vec<JobLine> {
        self.jobs
            .iter()
            .find(|(id, _)| id == helper)
            .map(|(_, jobs)| jobs.clone())
            .unwrap_or_default()
    }

    async fn open_job(&mut self, helper: &HelperId) -> Result<SessionId, HelperRefusal> {
        self.seen.lock().expect("seen").opened.push(helper.clone());
        Ok(self.job_session.clone())
    }

    async fn next_told(&mut self, watching: &[HelperId]) -> ToldWire {
        match self.told.iter().position(|t| watching.contains(&t.helper)) {
            Some(at) => self.told.remove(at).expect("told"),
            None => std::future::pending().await,
        }
    }
}

/// The helpers of a person: Inbox and Research in the desktop Space, Garden in another.
fn three() -> Vec<HelperRow> {
    vec![
        row(1, "Inbox", "desktop", "s-60"),
        row(2, "Research", "desktop", "s-61"),
        row(3, "Garden", "home", "s-62"),
    ]
}

/// A session the companion opened for a helper (not the editor), in the log.
async fn seed(log: &MemoryLog, id: &str) {
    let opening = Opening {
        task: TaskId::parse(&format!("t-{id}")).expect("task"),
        space: SpaceId::desktop(),
        opener: Some(app("org.quire.Companion")),
        agent: Some(AgentRef::Companion),
        backend: BackendKind::Native,
        parent: None,
        forked_from: None,
        cwd: None,
        started_from: None,
        label: None,
        rewind: Default::default(),
    };
    log.append(&session(id), Seq(0), &SessionEntry::Opened(opening))
        .await
        .expect("seed");
}

async fn seeded() -> Arc<MemoryLog> {
    let log = Arc::new(MemoryLog::new());
    for id in ["s-60", "s-61", "s-62", "s-70"] {
        seed(&log, id).await;
    }
    log
}

async fn load(ed: &mut Editor, id: &str) -> Value {
    ed.call(
        "session/load",
        json!({"sessionId": id, "cwd": "/work/a", "mcpServers": []}),
        "LoadSessionRequest",
        "LoadSessionResponse",
    )
    .await
}

fn texts(updates: &[Value], kind: &str) -> Vec<String> {
    updates
        .iter()
        .filter(|u| u["sessionUpdate"] == kind)
        .map(|u| u["content"]["text"].as_str().unwrap_or("").to_owned())
        .collect()
}

#[tokio::test]
async fn list_shows_the_helpers_of_the_workspaces_space_beside_the_companions_sessions() {
    let log = seeded().await;
    let (fake, _) = FakeHelpers::new(three());
    let (mut server, mut ed) = rig_with(&log, vec![], EDITOR, fake);
    let script = async move {
        ed.initialize().await;
        let companion = ed.new_session("/work/a").await;
        let all = ed
            .call(
                "session/list",
                json!({"cwd": "/work/a"}),
                "ListSessionsRequest",
                "ListSessionsResponse",
            )
            .await;
        let rows = all["result"]["sessions"].as_array().expect("sessions");
        let seen: Vec<(&str, Option<&str>)> = rows
            .iter()
            .map(|r| (r["sessionId"].as_str().unwrap(), r["title"].as_str()))
            .collect();
        assert_eq!(
            seen,
            [
                ("s-60", Some("Inbox")),
                ("s-61", Some("Research")),
                (companion.as_str(), None),
            ]
        );
    };
    let (ran, ()) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
}

#[tokio::test]
async fn a_list_without_a_workspace_shows_no_helpers() {
    let log = seeded().await;
    let (fake, _) = FakeHelpers::new(three());
    let (mut server, mut ed) = rig_with(&log, vec![], EDITOR, fake);
    let script = async move {
        ed.initialize().await;
        let all = ed
            .call(
                "session/list",
                json!({}),
                "ListSessionsRequest",
                "ListSessionsResponse",
            )
            .await;
        assert_eq!(all["result"]["sessions"], json!([]));
    };
    let (ran, ()) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
}

#[tokio::test]
async fn new_with_a_helper_in_meta_opens_a_job_and_a_plain_new_stays_the_companion() {
    let log = seeded().await;
    let (fake, seen) = FakeHelpers::new(three());
    let (mut server, mut ed) = rig_with(&log, vec![], EDITOR, fake);
    let script = async move {
        ed.initialize().await;
        let job = ed
            .call(
                "session/new",
                json!({"cwd": "/work/a", "mcpServers": [], "_meta": {"helper": "Research"}}),
                "NewSessionRequest",
                "NewSessionResponse",
            )
            .await;
        assert_eq!(job["result"]["sessionId"], "s-70");
        let plain = ed.new_session("/work/a").await;
        assert_eq!(plain, "s-1");
        // A helper of another Space, or none by that name, is turned away in plain words.
        for name in ["Garden", "Nobody"] {
            let params = json!({"cwd": "/work/a", "mcpServers": [], "_meta": {"helper": name}});
            let id = ed.send("session/new", params, "NewSessionRequest").await;
            let reply = ed
                .until_reply(id, "NewSessionResponse", &mut |_| None)
                .await;
            assert_eq!(
                reply["error"]["message"],
                format!("There is no helper called {name} here.")
            );
        }
    };
    let (ran, ()) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
    assert_eq!(seen.lock().expect("seen").opened, [helper(2)]);
}

#[tokio::test]
async fn a_prompt_in_a_helpers_job_session_is_the_persons_turn_with_the_editors_caps() {
    let log = seeded().await;
    let (fake, _) = FakeHelpers::new(three());
    let turn = vec![vec![vec![
        words("Comparing."),
        started(1, "mail.archive", Effect::UndoableWrite),
        step(1, "mail.archive", Effect::UndoableWrite, done("moved")),
        end(TurnEnd::Done),
    ]]];
    let (mut server, mut ed) = rig_with(&log, turn, EDITOR, fake);
    let script = async move {
        ed.initialize().await;
        let job = ed
            .call(
                "session/new",
                json!({"cwd": "/work/a", "mcpServers": [], "_meta": {"helper": "Research"}}),
                "NewSessionRequest",
                "NewSessionResponse",
            )
            .await;
        assert_eq!(job["result"]["sessionId"], "s-70");
        let reply = ed
            .prompt("s-70", "compare the three offers", &mut allow)
            .await;
        assert_eq!(reply["result"]["stopReason"], "end_turn");
        // Editor caps: the editor's own prompt still stands before a change.
        assert_eq!(ed.permission_requests().len(), 1);
    };
    let (ran, ()) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
    let host = server.into_host();
    assert_eq!(host.turns.len(), 1);
    let (at, turn) = &host.turns[0];
    assert_eq!(at, &session("s-70"));
    assert_eq!(turn.text, "compare the three offers");
    assert!(matches!(&turn.from, TurnSource::Editor(a) if *a == app(EDITOR)));
}

#[tokio::test]
async fn a_prompt_in_a_helpers_conversation_goes_to_the_helper_and_streams_its_words_back() {
    let log = seeded().await;
    let (fake, seen) = FakeHelpers::new(three());
    let (mut server, mut ed) = rig_with(&log, vec![], EDITOR, fake);
    let script = async move {
        ed.initialize().await;
        load(&mut ed, "s-61").await;
        let reply = ed
            .prompt("s-61", "compare the three offers", &mut allow)
            .await;
        assert_eq!(reply["result"]["stopReason"], "end_turn");
        // The helper's words are one plain agent line, and the editor was asked nothing.
        assert_eq!(
            texts(&ed.updates(), "agent_message_chunk"),
            ["Offer B is the best."]
        );
        assert!(ed.permission_requests().is_empty());
    };
    let (ran, ()) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
    // The turn did not run in this host: it went through the seam, as the editor's words.
    let said = seen.lock().expect("seen").said.clone();
    assert_eq!(said.len(), 1);
    assert_eq!(said[0].0, session("s-61"));
    assert_eq!(said[0].1.text, "compare the three offers");
    assert!(matches!(&said[0].1.from, TurnSource::Editor(a) if *a == app(EDITOR)));
    assert!(server.into_host().turns.is_empty());
}

#[tokio::test]
async fn a_helper_that_cannot_answer_in_its_conversation_is_a_plain_error() {
    let log = seeded().await;
    let (mut fake, _) = FakeHelpers::new(three());
    fake.answer = Err("Research needs a model before you can talk with it.".to_owned());
    let (mut server, mut ed) = rig_with(&log, vec![], EDITOR, fake);
    let script = async move {
        ed.initialize().await;
        load(&mut ed, "s-61").await;
        let params = json!({"sessionId": "s-61", "prompt": [{"type": "text", "text": "hello"}]});
        let id = ed.send("session/prompt", params, "PromptRequest").await;
        let reply = ed.until_reply(id, "PromptResponse", &mut |_| None).await;
        assert_eq!(
            reply["error"]["message"],
            "Research needs a model before you can talk with it."
        );
    };
    let (ran, ()) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
}

#[tokio::test]
async fn load_replays_the_recent_jobs_in_plain_words() {
    let log = seeded().await;
    let (mut fake, _) = FakeHelpers::new(three());
    fake.jobs.push((
        helper(2),
        vec![JobLine {
            id: JobId(1),
            goal: "compare the three offers".to_owned(),
            state: JobShown::Done,
            note: None,
            answer: Some("Offer B is the best.".to_owned()),
            from: Default::default(),
        }],
    ));
    let (mut server, mut ed) = rig_with(&log, vec![], EDITOR, fake);
    let script = async move {
        ed.initialize().await;
        load(&mut ed, "s-61").await;
        let updates = ed.updates();
        assert_eq!(
            texts(&updates, "user_message_chunk"),
            ["compare the three offers"]
        );
        assert_eq!(
            texts(&updates, "agent_message_chunk"),
            ["Research finished this.", "Offer B is the best."]
        );
    };
    let (ran, ()) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
}

#[tokio::test]
async fn a_board_change_streams_as_a_plain_line_in_that_helpers_session() {
    let log = seeded().await;
    let (mut fake, _) = FakeHelpers::new(three());
    let line = "Research took the job 'compare the three offers'";
    fake.told.push_back(ToldWire {
        n: 1,
        helper: helper(2),
        text: line.to_owned(),
    });
    let (mut server, mut ed) = rig_with(&log, vec![], EDITOR, fake);
    let script = async move {
        ed.initialize().await;
        load(&mut ed, "s-61").await;
        let pushed = ed.recv().await;
        assert_eq!(pushed["method"], "session/update");
        assert_eq!(pushed["params"]["sessionId"], "s-61");
        let update = &pushed["params"]["update"];
        assert_eq!(update["sessionUpdate"], "agent_message_chunk");
        assert_eq!(update["content"]["text"], line);
    };
    let (ran, ()) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
}

fn files_scope() -> StandingScope {
    StandingScope::Files {
        action: action("mail.archive"),
        under: AbsPath::parse("/home/u/docs").expect("path"),
    }
}

fn sheet(id: &str) -> BackendEvent {
    let request = ConfirmRequest::new(ConfirmParts {
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
    .offering(AlwaysOffer::Offered(files_scope()));
    BackendEvent::Confirm(Box::new(request))
}

/// Opens a job session for Research (`s-70`): a job still runs in the editor's own host.
async fn open_job(ed: &mut Editor) {
    let job = ed
        .call(
            "session/new",
            json!({"cwd": "/work/a", "mcpServers": [], "_meta": {"helper": "Research"}}),
            "NewSessionRequest",
            "NewSessionResponse",
        )
        .await;
    assert_eq!(job["result"]["sessionId"], "s-70");
}

#[tokio::test]
async fn allow_always_in_a_helpers_job_session_reaches_the_host_as_always() {
    let log = seeded().await;
    let (fake, _) = FakeHelpers::new(three());
    let turn = vec![vec![vec![sheet("c-1"), end(TurnEnd::Done)]]];
    let (mut server, mut ed) = rig_with(&log, turn, EDITOR, fake);
    let script = async move {
        ed.initialize().await;
        open_job(&mut ed).await;
        let mut always = |_: &Value| {
            Some(json!({"outcome": {"outcome": "selected", "optionId": "allow_always"}}))
        };
        ed.prompt("s-70", "archive them", &mut always).await;
    };
    let (ran, ()) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
    // The router records the grant under the helper's id, so the host is told what was chosen.
    let id = ConfirmId::parse("c-1").expect("id");
    assert_eq!(server.into_host().sheets, vec![(id, SheetChoice::Always)]);
}

#[tokio::test]
async fn a_refusal_from_the_helpers_app_in_a_job_reaches_the_editor_with_the_apps_own_sentence() {
    let own = "That folder is locked by another account.";
    let log = seeded().await;
    let (fake, _) = FakeHelpers::new(three());
    let turn = vec![vec![vec![
        started(1, "mail.archive", Effect::UndoableWrite),
        step(
            1,
            "mail.archive",
            Effect::UndoableWrite,
            StepEnd::Refused(CallRefusal::App(AppRefusal::Failed(FailText(
                own.to_owned(),
            )))),
        ),
        end(TurnEnd::Done),
    ]]];
    let (mut server, mut ed) = rig_with(&log, turn, EDITOR, fake);
    let script = async move {
        ed.initialize().await;
        open_job(&mut ed).await;
        ed.prompt("s-70", "archive them", &mut allow).await;
        let failed: Vec<Value> = ed
            .updates()
            .into_iter()
            .filter(|u| u["status"] == "failed")
            .collect();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0]["content"][0]["content"]["text"], own);
    };
    let (ran, ()) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
}
