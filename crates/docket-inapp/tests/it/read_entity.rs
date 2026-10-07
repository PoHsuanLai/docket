//! A thing the planner holds (a thread on screen) is a handle but not text. `quire_read` on it
//! is told to the planner as a line that names the kind and how to get its text; the planner
//! reads the thread with the app's own action (a call the policy sees), then `quire_read` on the
//! handle that returns is answered by the reader. Every model is scripted, the mail is an
//! in-process fake, the clock is virtual.

use crate::support::TestSheet;
use crate::support::host::{clock, mail, work};
use crate::support::infer::{ScriptedInfer, call, words};
use docket_client::IntentProvider;
use docket_core::{
    AppRefusal, ChoiceId, EntityRef, Handle, Hit, Invocation, Outcome, Preview, ReaderAsk,
    ReaderTask, SuggestAsk, TargetValue, UndoFault, UndoToken, ValidManifest, Value, ValueSchema,
};
use docket_fake::{FakeMail, MAIL_MANIFEST};
use docket_inapp::{Ending, InAppAgent, InAppKit, InAppParts, TransportReader};
use docket_router::parse;
use porter_core::AppName;
use prov::{ActionName, Actor, EntityId, EntityKey, EntityKind, Labelled};
use serde_json::{Value as Json, json};

const READ: &str = "org.quire.Mail-mail.thread.read";
const SEARCH_CALL: &str = "org.quire.Mail-mail.thread.search";

/// The fixture mail app with one more action, `mail.thread.search`, which answers every thread
/// as a thing (the fixture manifest has none): the way a planner comes to hold a thread.
struct Searching {
    mail: FakeMail,
    manifest: ValidManifest,
}

const SEARCH: &str = r#"
[[actions]]
name = "mail.thread.search"
label = "Find threads"
on = { kind = "nothing" }
effect = "read"
classes = ["mail"]
undo = "not_undoable"
reach = "offered"
latency = "quick"
result = { kind = "entities", v = "mail.thread" }
keys = { kind = "none" }
lasting = "no"
dry_run = "none"

[[actions.params]]
name = "query"
label = "Words in the subject"
ty = { kind = "text", v = { max = 200, lines = "one" } }
need = { kind = "required" }
sink = "inert"
"#;

fn id(app: &AppName, key: &str) -> EntityId {
    EntityId {
        app: app.clone(),
        kind: EntityKind::parse("mail.thread").expect("kind"),
        key: EntityKey::parse(key).expect("key"),
    }
}

fn searching() -> Searching {
    let text = format!("{MAIL_MANIFEST}\n{SEARCH}");
    Searching {
        mail: mail(),
        manifest: parse(&text).expect("manifest with search"),
    }
}

impl IntentProvider for Searching {
    fn manifest(&self) -> &ValidManifest {
        &self.manifest
    }

    async fn perform(&self, inv: Invocation) -> Result<Outcome, AppRefusal> {
        if inv.action.as_str() != "mail.thread.search" {
            return self.mail.perform(inv).await;
        }
        let app = AppName::parse("org.quire.Mail").expect("app");
        let read = Invocation {
            action: ActionName::parse("mail.thread.read").expect("action"),
            target: TargetValue::Entities(vec![id(&app, "t1")]),
            ..inv
        };
        let mut out = self.mail.perform(read).await?;
        let found = ["t1", "t2"].map(|key| id(&app, key)).to_vec();
        out.value = out.value.map(|held| Labelled {
            value: Value::Entities(found),
            label: held.label,
        });
        Ok(out)
    }

    async fn dry_run(&self, inv: Invocation) -> Result<Preview, AppRefusal> {
        self.mail.dry_run(inv).await
    }

    async fn undo(&self, token: UndoToken, actor: Actor) -> Result<(), UndoFault> {
        self.mail.undo(token, actor).await
    }

    async fn search(&self, text: &str) -> Vec<Hit> {
        self.mail.search(text).await
    }

    async fn preview(&self, id: &EntityId) -> Preview {
        self.mail.preview(id).await
    }

    async fn suggest(&self, ask: SuggestAsk) -> Vec<EntityRef> {
        self.mail.suggest(ask).await
    }
}

fn classify(input: u64) -> Json {
    let ask = ReaderAsk {
        inputs: vec![Handle(input)],
        want: ValueSchema::Choice(vec![
            ChoiceId::parse("invoice").expect("choice"),
            ChoiceId::parse("newsletter").expect("choice"),
        ]),
        task: ReaderTask::Classify,
    };
    serde_json::to_value(&ask).expect("ask")
}

#[tokio::test]
async fn a_thread_is_read_first_and_its_text_handle_is_then_answered() {
    let planner = ScriptedInfer::new(vec![
        call(SEARCH_CALL, json!({ "query": "Invoice" })),
        call("quire_read", classify(1)),
        call(READ, json!({ "target": { "handle": 1 } })),
        call("quire_read", classify(3)),
        words("It is an invoice."),
    ]);
    let reader = ScriptedInfer::new(vec![words("invoice")]);
    let sheet = TestSheet::answering(vec![docket_inapp::SheetAnswer::Once; 2]);
    let parts = InAppParts {
        provider: searching(),
        context: crate::support::Nowhere,
        sheet,
        reviewer: docket_fake::ScriptedReviewer::always_allow(),
        model: planner.clone(),
        clock: clock(),
        space: work(),
        config: docket_core::AgentConfig::default(),
    };
    let mut agent = InAppAgent::with_kit(
        parts,
        InAppKit::default().reader(TransportReader::in_process(reader.clone())),
    )
    .expect("agent");
    let reply = agent.ask("what kind of mail is this").await.expect("turn");
    assert_eq!(reply.ending, Ending::Done, "{reply:?}");
    let told = planner.user_text(2);
    assert!(
        told.contains("#1 is a mail.thread, not text"),
        "the fault line names the handle and kind: {told}"
    );
    assert!(told.contains("mail.thread.read"), "{told}");
    assert_eq!(
        reader.asked().len(),
        1,
        "only the text handle reached the reader"
    );
    assert!(planner.user_text(4).contains("invoice"), "answered");
}
