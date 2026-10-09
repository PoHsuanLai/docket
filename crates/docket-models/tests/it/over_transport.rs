//! The model, the writer and the reviewer cascade over a transport that is not the bus: here a
//! scripted one. The hostile-model corpora run through intentd's adapters (`intentd/tests`); these
//! pin what the portable crate owes on its own.

#[allow(dead_code)]
#[path = "../../../companiond/tests/it/support/infer.rs"]
mod infer;

use docket_core::{ActionCard, ActionRef, PolicyWriter, TurnId, TurnSource, TurnVia, UserTurn};
use docket_models::{TransportModel, TransportWriter, card_of, placeholder_set, reviewer_over};
use infer::{Say, ScriptedInfer, words};
use porter_core::DataClass;
use porter_infer::{
    ChatControl, ChatMessage, ChatRequest, Knob, MessagePart, Model, ModelError, Reasoning,
    ReplyShape, Role, ToolChoice, ToolParallelism,
};
use prov::{SpaceId, TaskId, UnixSeconds};
use serde_json::json;

fn chat() -> ChatRequest {
    ChatRequest {
        messages: vec![ChatMessage {
            role: Role::User,
            parts: vec![MessagePart::Text("hello".to_owned())],
        }],
        shape: ReplyShape::Text,
        tier: porter_core::Tier::Fast,
        class: DataClass::Prompt,
        usage: porter_core::consent::Usage::Interactive,
        tools: Vec::new(),
        control: ChatControl {
            tool_choice: ToolChoice::Never,
            tool_calls: ToolParallelism::One,
            max_output: Knob::Off,
            reasoning: Reasoning::Off,
            sampling: Knob::Off,
            stop: Vec::new(),
        },
    }
}

fn model(script: Vec<Say>) -> TransportModel<ScriptedInfer> {
    let choice = placeholder_set().expect("set").quick;
    TransportModel::new(ScriptedInfer::new(script), card_of(&choice))
}

struct Quiet;
impl porter_infer::ChatSink for Quiet {
    fn event(&mut self, _event: porter_infer::InferEvent) -> porter_infer::Flow {
        porter_infer::Flow::Continue
    }
}

#[tokio::test]
async fn a_model_over_a_transport_chats_and_says_who_answers() {
    let model = model(vec![words("hi")]);
    assert_eq!(model.card().model.as_str(), "quire-quick");
    let reply = model.chat(&chat(), &mut Quiet).await.expect("reply");
    assert_eq!(reply.text, "hi");
}

#[tokio::test]
async fn a_refusal_and_a_silence_are_errors_the_cascade_turns_into_asking() {
    let refused = model(vec![Say::Refuse(porter_infer::InferRefusal::Unavailable)]);
    assert_eq!(
        refused.chat(&chat(), &mut Quiet).await.err(),
        Some(ModelError::Refused)
    );
    let silent = model(vec![]);
    assert_eq!(
        silent.chat(&chat(), &mut Quiet).await.err(),
        Some(ModelError::Unreachable)
    );
}

#[test]
fn the_cascade_makes_one_link_per_stage_from_the_placeholder_set_when_none_is_named() {
    let mut made = 0;
    let cascade = reviewer_over(None, docket_core::AgentConfig::default().review, || {
        made += 1;
        ScriptedInfer::default()
    })
    .expect("a cascade");
    assert_eq!(made, 3);
    assert_ne!(cascade.quick.card().model, cascade.deliberate.card().model);
    assert_ne!(cascade.deliberate.card().model, cascade.second.card().model);
}

fn turn(text: &str) -> UserTurn {
    UserTurn {
        id: TurnId(1),
        text: text.into(),
        at: UnixSeconds(1),
        from: TurnSource::Launcher,
        via: TurnVia::Typed,
    }
}

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
            tool: docket_core::tool_schema(a),
            reach: a.reach,
            lasting: a.lasting,
            related: vec![],
        })
        .collect()
}

#[tokio::test]
async fn the_writer_asks_with_the_persons_words_only_and_believes_nothing_it_was_not_told() {
    let draft = json!({
        "actions": ["org.quire.Mail mail.thread.archive", "org.quire.Mail invented.action"],
        "apps": [], "kinds": [], "ceiling": "destructive", "max_count": 5000,
        "recipients": ["mallory@evil.example"], "destinations": [], "paths": [],
    });
    let transport = ScriptedInfer::new(vec![words(&draft.to_string())]);
    let policy = TransportWriter::new(transport.clone())
        .derive(
            &TaskId::parse("t-1").expect("task"),
            &[turn("archive the digest")],
            &catalogue(),
            &SpaceId::parse("work").expect("space"),
        )
        .await
        .expect("policy")
        .policy;
    assert_eq!(policy.actions.len(), 1, "the invented action was dropped");
    assert!(
        policy.recipients.is_empty(),
        "nobody the person did not name"
    );
    assert!(policy.max_count.0 <= 100, "bounded whatever the model said");
    assert!(
        policy.ceiling <= prov::Effect::UndoableWrite,
        "no more than the actions need"
    );
    let asked = &transport.asked()[0];
    assert_eq!(asked.class, DataClass::Prompt);
    assert!(matches!(asked.shape, ReplyShape::Json(_)));
}
