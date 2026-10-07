//! The planner: its tools from the installed manifests, its request a function of the view, and
//! its reply read into what the loop does next. And the arguments of a tool call, read by the
//! types the action declares.

mod support;

use agent_loop::ModelOutput;
use companiond::*;
use docket_core::*;
use porter_core::AppName;
use prov::{ActionName, EntityId, EntityKey, EntityKind, Integrity, Source};
use serde_json::json;
use support::infer::{Say, call, words};
use support::planner_view::{catalogue, planner, view};

/// A value outside what the parameter declares.
fn wrong(name: &str) -> ArgsFault {
    ArgsFault::Wrong {
        param: ParamName::parse(name).expect("param"),
        why: docket_core::Why::Range,
    }
}

fn mail_tool(name: &str) -> CatalogueTool {
    catalogue()
        .tools()
        .iter()
        .find(|t| t.decl.name.as_str() == name)
        .cloned()
        .expect("tool")
}

#[test]
fn only_actions_an_agent_may_offer_are_tools_with_unique_short_names() {
    let c = catalogue();
    let names: Vec<&str> = c.tools().iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"org.quire.Mail-mail.thread.read"));
    assert!(names.contains(&"org.quire.Memory-memory.recall"));
    assert!(
        !names.iter().any(|n| n.contains("memory.forget")),
        "hidden: {names:?}"
    );
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len(), "unique");
    assert!(names.iter().all(|n| n.len() <= 64));
    // The registry's order: mail first.
    assert_eq!(names[0], "org.quire.Mail-mail.thread.read");
    assert_eq!(c.cards().len(), names.len());
}

#[test]
fn a_tool_names_its_target_by_what_the_action_acts_on() {
    let property = |name: &str| {
        let decl = mail_tool(name).declaration().expect("declaration");
        let schema: serde_json::Value = serde_json::from_str(decl.params.0.as_str()).expect("json");
        schema["properties"].get("target").cloned()
    };
    assert_eq!(
        property("mail.thread.read").expect("one thing")["anyOf"][0]["properties"]["kind"]["const"],
        "mail.thread"
    );
    assert_eq!(
        property("mail.thread.archive").expect("many things")["type"],
        "array"
    );
    assert!(property("mail.draft.create").is_none(), "acts on nothing");
}

fn entity(key: &str) -> EntityId {
    EntityId {
        app: AppName::parse("org.quire.Mail").expect("app"),
        kind: EntityKind::parse("mail.thread").expect("kind"),
        key: EntityKey::parse(key).expect("key"),
    }
}

fn param(name: &str) -> ParamName {
    ParamName::parse(name).expect("param")
}

/// A tool with one parameter of `ty`, to read arguments by.
fn tool_with(ty: ParamType, need: ParamNeed) -> CatalogueTool {
    let mut tool = mail_tool("mail.draft.create");
    tool.decl.params = vec![ParamDecl {
        name: param("p"),
        label: LabelText::parse("P").expect("label"),
        ty,
        need,
        sink: ArgSink::Inert,
    }];
    tool
}

fn read(ty: ParamType, json: serde_json::Value) -> Result<Value, ArgsFault> {
    let tool = tool_with(ty, ParamNeed::Required);
    read_call(&tool, &json!({ "p": json })).map(|c| c.args[&param("p")].value.clone())
}

#[test]
fn each_declared_type_reads_its_json() {
    use ParamType as T;
    let text = T::Text {
        max: CharCount(10),
        lines: Lines::One,
    };
    let choice = T::Choice(vec![ChoiceDecl {
        id: ChoiceId::parse("a").expect("id"),
        label: LabelText::parse("A").expect("label"),
    }]);
    let kind = EntityKind::parse("mail.thread").expect("kind");
    let thread = json!({ "app": "org.quire.Mail", "kind": "mail.thread", "key": "t1" });
    let cases = [
        (
            "text",
            text.clone(),
            json!("hi"),
            Ok(Value::Text("hi".into())),
        ),
        (
            "a handle for text",
            text,
            json!({ "handle": 3 }),
            Ok(Value::Handle(Handle(3))),
        ),
        (
            "url",
            T::Url,
            json!("https://x.test"),
            Ok(Value::Url("https://x.test".into())),
        ),
        (
            "integer in range",
            T::Integer { min: 1, max: 5 },
            json!(3),
            Ok(Value::Integer(3)),
        ),
        (
            "integer out of range",
            T::Integer { min: 1, max: 5 },
            json!(9),
            Err(wrong("p")),
        ),
        (
            "datetime",
            T::DateTime,
            json!(86_400),
            Ok(Value::DateTime(prov::UnixSeconds(86_400))),
        ),
        (
            "duration",
            T::Duration,
            json!(60),
            Ok(Value::Duration(Seconds(60))),
        ),
        (
            "date",
            T::Date,
            json!({ "year": 2026, "month": 10, "day": 3 }),
            Ok(Value::Date(CivilDate {
                year: 2026,
                month: 10,
                day: 3,
            })),
        ),
        (
            "a choice of the declared ones",
            choice.clone(),
            json!("a"),
            Ok(Value::Choice(ChoiceId::parse("a").expect("id"))),
        ),
        (
            "a choice that is not declared",
            choice,
            json!("zzz"),
            Err(wrong("p")),
        ),
        (
            "an entity of its kind",
            T::Entity(kind.clone()),
            thread.clone(),
            Ok(Value::Entity(entity("t1"))),
        ),
        (
            "an entity of another kind",
            T::Entity(EntityKind::parse("mail.contact").expect("kind")),
            thread.clone(),
            Err(wrong("p")),
        ),
        (
            "entities",
            T::Entities(kind.clone()),
            json!([thread.clone()]),
            Ok(Value::Entities(vec![entity("t1")])),
        ),
        (
            "entities with a handle among them",
            T::Entities(kind),
            json!([thread, { "handle": 2 }]),
            Ok(Value::List(vec![
                Value::Entity(entity("t1")),
                Value::Handle(Handle(2)),
            ])),
        ),
    ];
    for (name, ty, json, want) in cases {
        assert_eq!(read(ty, json), want, "{name}");
    }
}

#[test]
fn a_call_is_read_whole_or_not_at_all() {
    let required = tool_with(ParamType::Date, ParamNeed::Required);
    let optional = tool_with(ParamType::Date, ParamNeed::Optional);
    assert_eq!(
        read_call(&required, &json!({})),
        Err(ArgsFault::Missing(param("p")))
    );
    assert!(
        read_call(&optional, &json!({}))
            .expect("optional")
            .args
            .is_empty()
    );
    assert_eq!(
        read_call(&required, &json!({ "q": 1 })),
        Err(ArgsFault::Unknown("q".into()))
    );
    assert_eq!(
        read_call(&required, &json!(["x"])),
        Err(ArgsFault::NotAnObject)
    );
    // Whatever the model passes is its own, untrusted: the router derives the real labels.
    let call = read_call(
        &tool_with(ParamType::Url, ParamNeed::Required),
        &json!({ "p": "https://x.test" }),
    )
    .expect("call");
    let label = &call.args[&param("p")].label;
    assert_eq!(label.integrity, Integrity::Untrusted);
    assert!(
        label
            .sources
            .contains(&Source::Model(prov::ModelRole::Planner))
    );
    // The target follows `on`.
    let archive = mail_tool("mail.thread.archive");
    let one = json!({ "app": "org.quire.Mail", "kind": "mail.thread", "key": "t1" });
    assert_eq!(
        read_call(&archive, &json!({ "target": [one.clone()] }))
            .expect("many")
            .target,
        TargetValue::Entities(vec![entity("t1")])
    );
    assert!(
        read_call(&archive, &json!({ "target": one })).is_err(),
        "a list is wanted"
    );
    assert!(
        read_call(
            &mail_tool("mail.draft.create"),
            &json!({ "body": "x", "target": [] })
        )
        .is_err(),
        "an action on nothing names no target"
    );
}

#[tokio::test]
async fn the_request_is_the_view_in_two_messages_and_the_tools_of_the_cards() {
    let (planner, _) = planner(vec![]);
    let view = view(catalogue().cards());
    let request = planner.request(&view);
    assert_eq!(request.messages.len(), 2);
    let system = system_text(&view);
    assert!(system.starts_with(RULES));
    assert!(system.contains("Prefers short answers"));
    assert!(system.contains("Eve is the landlord"));
    let names: Vec<&str> = request.tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"org.quire.Mail-mail.thread.read"));
    assert_eq!(
        &names[names.len() - 3..],
        [TOOL_ASK, TOOL_READ, TOOL_FINISH]
    );
    // The same view gives the same request, byte for byte.
    assert_eq!(request, planner.request(&view));
    // A card the budget cut is not a tool.
    let some = view_of_first_card();
    let fewer = planner.request(&some);
    assert_eq!(fewer.tools.len(), 1 + 3);
}

fn view_of_first_card() -> PlannerView {
    view(catalogue().cards().into_iter().take(1).collect())
}

#[tokio::test]
async fn a_reply_is_read_into_calls_a_read_a_question_an_end_or_words() {
    let thread = json!({ "app": "org.quire.Mail", "kind": "mail.thread", "key": "t1" });
    let (planner, _) = planner(vec![
        // Words and a call: both come back.
        Say::Reply(
            "Reading it.".into(),
            vec![(
                "org.quire.Mail-mail.thread.read".into(),
                json!({ "target": thread }),
            )],
        ),
        // A tool that is not offered, beside one that is.
        Say::Reply(
            String::new(),
            vec![
                ("nothing-here".into(), json!({})),
                (
                    "org.quire.Mail-mail.thread.read".into(),
                    json!({ "target": thread }),
                ),
            ],
        ),
        call(
            TOOL_ASK,
            json!({ "text": "Which one?", "choices": ["a", "b"] }),
        ),
        call(
            TOOL_READ,
            json!({ "inputs": [1], "task": "classify", "want": { "kind": "choice", "v": ["spam", "ham"] } }),
        ),
        call(TOOL_FINISH, json!({})),
        words("Just words."),
        // Only an unknown tool, only bad arguments, nothing at all, and words cut off.
        call("nothing-here", json!({})),
        call("org.quire.Mail-mail.thread.read", json!({ "target": 5 })),
        words(""),
        Say::Cut("Half of a sen".into()),
    ]);
    let view = view(catalogue().cards());

    let reply = planner.converse(&view).await.expect("calls");
    assert_eq!(reply.said.as_deref(), Some("Reading it."));
    let ModelOutput::Calls(calls) = &reply.then else {
        panic!("{:?}", reply.then)
    };
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].call.origin, Origin::Companion);
    assert_eq!(
        calls[0].call.action.name,
        ActionName::parse("mail.thread.read").expect("action")
    );
    assert_eq!(calls[0].tier, agent_loop::Tier::Typed);
    assert_eq!(
        calls[0].call.target,
        TargetValue::Entities(vec![entity("t1")])
    );
    assert!(reply.served.is_some());

    let ModelOutput::Calls(calls) = planner.plan(&view).await.expect("one valid") else {
        panic!("calls")
    };
    assert_eq!(calls.len(), 1, "the tool nobody offered was dropped");

    assert_eq!(
        planner.plan(&view).await,
        Ok(ModelOutput::Ask {
            text: "Which one?".into(),
            choices: vec!["a".into(), "b".into()]
        })
    );
    let ModelOutput::Read(ask) = planner.plan(&view).await.expect("read") else {
        panic!("read")
    };
    assert_eq!(ask.inputs, vec![Handle(1)]);
    assert_eq!(ask.task, ReaderTask::Classify);
    assert_eq!(planner.plan(&view).await, Ok(ModelOutput::Finish));
    assert_eq!(
        planner.plan(&view).await,
        Ok(ModelOutput::Say("Just words.".into()))
    );
    // An unknown tool and bad arguments are told to the model; nothing at all, and words cut off,
    // are not a reply to tell anything about.
    assert_eq!(
        planner.plan(&view).await,
        Ok(ModelOutput::Unread(docket_core::ReplyFault::NoSuchTool(
            "nothing-here".into()
        )))
    );
    assert!(matches!(
        planner.plan(&view).await,
        Ok(ModelOutput::Unread(docket_core::ReplyFault::Args(_)))
    ));
    for why in ["nothing", "a cut-off reply"] {
        assert_eq!(
            planner.plan(&view).await,
            Err(PlanFault::Unreadable),
            "{why}"
        );
    }
    // The script is spent: no model to ask.
    assert_eq!(planner.plan(&view).await, Err(PlanFault::Unavailable));
}
