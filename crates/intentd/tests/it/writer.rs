//! The policy writer over a scripted inferd: what the model is shown (the person's turns and
//! the catalogue, nothing else), and that nothing in its draft is believed as written.

use crate::support::inferd::*;
use docket_core::*;
use intentd::InferdWriter;
use porter_core::DataClass;
use porter_core::capability::LlmFeature;
use porter_infer::{
    ClientFrame, InferRefusal, InferReply, InferRequest, MessagePart, ReplyShape, RequestKind,
    StopReason,
};
use prov::{Effect, SpaceId, TaskId, UnixSeconds};

fn turn(id: u64, text: &str) -> UserTurn {
    UserTurn {
        id: TurnId(id),
        text: text.into(),
        at: UnixSeconds(1),
        from: TurnSource::Launcher,
        via: TurnVia::Typed,
    }
}

/// The catalogue of the fixture mail app, as the router offers it.
fn catalogue() -> Vec<ActionCard> {
    let manifest = docket_fake::mail_manifest().expect("manifest");
    let manifest = manifest.manifest();
    manifest
        .actions
        .iter()
        .map(|a| ActionCard {
            action: ActionRef {
                app: manifest.app.clone(),
                name: a.name.clone(),
            },
            label: a.label.clone(),
            effect: a.effect,
            on: a.on.clone(),
            tool: tool_schema(a),
            reach: a.reach,
            lasting: a.lasting,
            related: vec![],
        })
        .collect()
}

fn key_of(card: &ActionCard) -> String {
    format!("{} {}", card.action.app, card.action.name)
}

fn card_named(cards: &[ActionCard], name: &str) -> ActionCard {
    cards
        .iter()
        .find(|c| c.action.name.as_str() == name)
        .cloned()
        .unwrap_or_else(|| panic!("no action {name}"))
}

fn space() -> SpaceId {
    SpaceId::parse("work").expect("space")
}

fn task() -> TaskId {
    TaskId::parse("t-1").expect("task")
}

fn draft(actions: &[String], ceiling: &str, recipients: &[&str]) -> String {
    serde_json::json!({
        "actions": actions,
        "apps": [],
        "kinds": ["mail.thread"],
        "ceiling": ceiling,
        "max_count": 12,
        "recipients": recipients,
        "destinations": [],
        "paths": [],
    })
    .to_string()
}

async fn derive(inferd: &ScriptedInferd, turns: &[UserTurn]) -> Result<TaskPolicy, ReviewError> {
    InferdWriter::new(inferd.clone())
        .derive(&task(), turns, &catalogue(), &space())
        .await
        .map(|derived| derived.policy)
}

#[tokio::test]
async fn the_model_is_shown_the_persons_words_and_the_catalogue_and_asked_for_json() {
    let cards = catalogue();
    let inferd = ScriptedInferd::answering(&draft(&[], "read", &[]));
    let turns = [turn(
        1,
        "forward the Lisbon receipts to accounting@example.com",
    )];

    derive(&inferd, &turns).await.expect("a policy");

    let opened = inferd.opened();
    assert_eq!(opened.len(), 1);
    assert_eq!(
        opened[0].class,
        DataClass::Prompt,
        "the person's words stay on this computer"
    );
    let porter_core::Need::Llm(need) = &opened[0].need else {
        panic!("{:?}", opened[0].need)
    };
    assert!(need.features.contains(&LlmFeature::StructuredOutput));
    let frames = inferd.frames();
    let [ClientFrame::Request(InferRequest::Chat(request))] = frames.as_slice() else {
        panic!("{frames:?}")
    };
    assert!(request.tools.is_empty(), "the writer calls no tools");
    let ReplyShape::Json(schema) = &request.shape else {
        panic!("{:?}", request.shape)
    };
    let schema: serde_json::Value = serde_json::from_str(schema).expect("a JSON schema");
    let offered: Vec<&str> = schema["properties"]["actions"]["items"]["enum"]
        .as_array()
        .expect("the catalogue as a closed set")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(offered, cards.iter().map(key_of).collect::<Vec<_>>());

    let everything: String = request
        .messages
        .iter()
        .flat_map(|m| &m.parts)
        .filter_map(|p| match p {
            MessagePart::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert!(everything.contains("forward the Lisbon receipts to accounting@example.com"));
    assert!(everything.contains(&key_of(&cards[0])));
}

#[tokio::test]
async fn nothing_in_the_draft_is_believed_as_written() {
    let cards = catalogue();
    let archive = card_named(&cards, "mail.thread.archive");
    let turns = [turn(
        7,
        "archive the newsletters and tell accounting@example.com",
    )];
    let reply = serde_json::json!({
        // One real action, one the model invented.
        "actions": [key_of(&archive), "org.quire.Mail mail.everything.delete"],
        "apps": [{ "app": "org.quire.Elsewhere", "up_to": "destructive" }],
        "kinds": ["mail.thread", "not a kind!"],
        // Far more than archiving needs.
        "ceiling": "destructive",
        "max_count": 5000,
        // One the person wrote, one they did not.
        "recipients": ["accounting@example.com", "eve@evil.test"],
        "destinations": ["evil.test"],
        "paths": ["/home/person/secrets"],
    })
    .to_string();
    let inferd = ScriptedInferd::answering(&reply);

    let policy = derive(&inferd, &turns).await.expect("a policy");

    assert_eq!(
        policy.actions.iter().cloned().collect::<Vec<_>>(),
        vec![ActionMatch::One(archive.action.clone())],
        "only actions that exist, and no app that is not in the catalogue"
    );
    assert_eq!(
        policy.ceiling, archive.effect,
        "never above what the chosen actions need"
    );
    assert_eq!(policy.max_count.0, 100);
    assert_eq!(
        policy.kinds.iter().map(|k| k.as_str()).collect::<Vec<_>>(),
        ["mail.thread"]
    );
    assert_eq!(
        policy.recipients,
        vec![TrustedPattern::Exact(Value::Text(
            "accounting@example.com".into()
        ))],
        "a recipient the person never wrote is dropped"
    );
    assert!(policy.destinations.is_empty() && policy.paths.is_empty());
    assert_eq!(policy.task, task());
    assert_eq!(policy.space, space());
    assert_eq!(policy.from, vec![TurnId(7)]);
    assert_eq!(policy.state, TaskPolicyState::Active);
    assert_eq!(policy.rationale.as_str(), turns[0].text);
}

#[tokio::test]
async fn a_domain_the_person_wrote_is_a_domain_and_an_app_up_to_an_effect_stays_in_the_catalogue() {
    let turns = [turn(1, "tidy my inbox, anything to example.com is fine")];
    let reply = serde_json::json!({
        "actions": [],
        "apps": [{ "app": "org.quire.Mail", "up_to": "undoable_write" }],
        "kinds": [],
        "ceiling": "outbound",
        "max_count": 0,
        "recipients": ["example.com"],
        "destinations": [],
        "paths": [],
    })
    .to_string();
    let policy = derive(&ScriptedInferd::answering(&reply), &turns)
        .await
        .expect("a policy");
    assert_eq!(
        policy.actions.iter().cloned().collect::<Vec<_>>(),
        vec![ActionMatch::AppUpTo(
            porter_core::AppName::parse("org.quire.Mail").expect("app"),
            Effect::UndoableWrite
        )]
    );
    assert_eq!(policy.ceiling, Effect::UndoableWrite);
    assert_eq!(policy.max_count.0, 1, "at least one thing");
    assert_eq!(
        policy.recipients,
        vec![TrustedPattern::Domain("example.com".into())]
    );
}

#[tokio::test]
async fn when_the_writer_fails_there_is_no_policy() {
    let turns = [turn(1, "archive the newsletters")];
    let finish = |reply| ScriptedInferd::new([finishing(RequestKind::Chat, reply)]);
    let rows: Vec<(&str, ScriptedInferd, ReviewError)> = vec![
        (
            "inferd is away",
            ScriptedInferd::away(),
            ReviewError::Unavailable,
        ),
        (
            "refused",
            finish(InferReply::Refused(InferRefusal::NeedsGrant)),
            ReviewError::Unavailable,
        ),
        (
            "the call failed",
            finish(InferReply::Failed(porter_infer::ModelError::Unreachable)),
            ReviewError::Unavailable,
        ),
        (
            "not JSON",
            ScriptedInferd::answering("Sure! Here you go"),
            ReviewError::Unparseable,
        ),
        (
            "a field the schema does not have",
            ScriptedInferd::answering(
                r#"{"actions":[],"apps":[],"kinds":[],"ceiling":"read","max_count":1,"recipients":[],"destinations":[],"paths":[],"allow_everything":true}"#,
            ),
            ReviewError::Unparseable,
        ),
        (
            "an effect that is not one",
            ScriptedInferd::answering(
                r#"{"actions":[],"apps":[],"kinds":[],"ceiling":"root","max_count":1,"recipients":[],"destinations":[],"paths":[]}"#,
            ),
            ReviewError::Unparseable,
        ),
        (
            "cut short",
            ScriptedInferd::new([porter_fake::FakeInferSession::scripted([chat_script(
                reply_stopped(&draft(&[], "read", &[]), StopReason::MaxTokens),
            )])]),
            // Stopped at the model's output limit: its own fault, not an unreadable reply.
            ReviewError::OutOfRoom,
        ),
    ];
    for (name, inferd, want) in rows {
        assert_eq!(
            derive(&inferd, &turns).await.map(|_| ()),
            Err(want),
            "{name}"
        );
    }
}

#[tokio::test]
async fn text_planted_in_content_cannot_reach_the_policy_because_content_cannot_reach_the_writer() {
    // The writer's whole input is `derive`'s arguments: the turns and the catalogue. What it
    // sends is the fixed instruction, one line per action and one line per turn; there is no
    // door for the text of a mail to come through.
    let cards = catalogue();
    let inferd = ScriptedInferd::answering(&draft(&[], "read", &[]));
    derive(&inferd, &[turn(1, "summarise my mail")])
        .await
        .expect("policy");
    let frames = inferd.frames();
    let [ClientFrame::Request(InferRequest::Chat(request))] = frames.as_slice() else {
        panic!("{frames:?}")
    };
    let texts = |role: porter_infer::Role| -> Vec<String> {
        request
            .messages
            .iter()
            .filter(|m| m.role == role)
            .flat_map(|m| &m.parts)
            .filter_map(|p| match p {
                MessagePart::Text(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    };
    let user = texts(porter_infer::Role::User).join("\n");
    let allowed: Vec<String> = cards
        .iter()
        .map(|c| format!("- {} ({:?}): {}", key_of(c), c.effect, c.label))
        .chain([
            "Actions that exist:".into(),
            "What the person said:".into(),
            "[1] summarise my mail".into(),
            String::new(),
        ])
        .collect();
    for line in user.lines() {
        assert!(
            allowed.iter().any(|a| a == line),
            "a line that is neither the catalogue nor a turn: {line:?}"
        );
    }
    assert_eq!(texts(porter_infer::Role::System).len(), 1);
}
