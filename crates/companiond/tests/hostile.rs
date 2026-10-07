//! The planner reads a model that misbehaves: a call left in the words, words without end, a
//! question past its schema, marks that reorder or hide. Every one ends as a typed fault or as
//! bounded, plain words, never as a call.

mod support;

use agent_loop::ModelOutput;
use companiond::*;
use serde_json::json;
use support::infer::{Say, call, words};
use support::planner_view::{catalogue, planner, view};

const READ: &str = "org.quire.Mail-mail.thread.read";

async fn plan(say: Say) -> Result<ModelOutput, PlanFault> {
    let (planner, _) = planner(vec![say]);
    planner.plan(&view(catalogue().cards())).await
}

#[tokio::test]
async fn a_call_written_as_words_is_a_fault_in_every_template() {
    let thread = json!({ "app": "org.quire.Mail", "kind": "mail.thread", "key": "t1" });
    let hermes = format!(
        "<tool_call>\n{}\n</tool_call>",
        json!({ "name": READ, "arguments": { "target": thread } })
    );
    let qwen = format!(
        "<tool_call>\n<function={READ}>\n<parameter=target>\nt1\n</parameter>\n</function>\n</tool_call>"
    );
    let words_with = |text: &str| format!("Sure, doing it now. {text}");
    for (name, text) in [
        ("hermes", hermes.clone()),
        ("qwen xml", qwen),
        ("hermes after words", words_with(&hermes)),
        (
            "mistral",
            "[TOOL_CALLS] [{\"name\": \"x\", \"arguments\": {}}]".into(),
        ),
        ("llama", "<|python_tag|>{\"name\": \"x\"}".into()),
        ("capitals", hermes.to_uppercase()),
        (
            "zero-width inside the tag",
            "<tool\u{200b}_call>{}</tool_call>".into(),
        ),
        (
            "bidi around it",
            "\u{202e}<tool_call>{}</tool_call>\u{202c}".into(),
        ),
        ("fullwidth brackets", "\u{ff1c}tool_call\u{ff1e}{}".into()),
    ] {
        assert_eq!(
            plan(words(&text)).await,
            Err(PlanFault::CallInText),
            "{name}"
        );
    }
}

#[tokio::test]
async fn words_about_tools_are_still_words() {
    for text in [
        "I would call the tool_call function if I could.",
        "The <b>tool</b> is ready.",
        "[calls] are made through the router",
    ] {
        assert_eq!(
            plan(words(text)).await,
            Ok(ModelOutput::Say(text.into())),
            "{text}"
        );
    }
}

#[tokio::test]
async fn markup_beside_real_calls_is_dropped_and_the_calls_stand() {
    let thread = json!({ "app": "org.quire.Mail", "kind": "mail.thread", "key": "t1" });
    let (planner, _) = planner(vec![Say::Reply(
        "<tool_call>{}</tool_call>".into(),
        vec![(READ.into(), json!({ "target": thread }))],
    )]);
    let reply = planner
        .converse(&view(catalogue().cards()))
        .await
        .expect("the real call");
    assert_eq!(reply.said, None, "the markup is not an answer");
    assert!(matches!(reply.then, ModelOutput::Calls(c) if c.len() == 1));
}

#[tokio::test]
async fn words_are_bounded_and_carry_no_reordering_or_hidden_marks() {
    let ModelOutput::Say(long) = plan(words(&"ab ".repeat(40_000))).await.expect("words") else {
        panic!("not words");
    };
    assert!(long.chars().count() <= 32_001, "{} characters", long.len());
    assert!(long.ends_with('\u{2026}'));
    let ModelOutput::Say(plain) = plan(words("done\u{202e}txet\u{202c} and\u{200b} \u{2060}all"))
        .await
        .expect("words")
    else {
        panic!("not words");
    };
    assert_eq!(plain, "donetxet and all");
    // Only marks: nothing left to say.
    assert_eq!(
        plan(words("\u{202e}\u{200b}")).await,
        Err(PlanFault::Unreadable)
    );
}

#[tokio::test]
async fn a_question_past_what_its_tool_allows_is_unreadable() {
    let bad = [
        json!({ "text": "Q".repeat(401) }),
        json!({ "text": "" }),
        json!({ "text": "   " }),
        json!({ "text": 7 }),
        json!({ "text": "ok", "choices": ["1", "2", "3", "4", "5", "6", "7"] }),
        json!({ "text": "ok", "choices": ["c".repeat(81)] }),
        json!({ "text": "ok", "choices": [1] }),
        json!({ "text": "ok", "choices": "a" }),
    ];
    for args in bad {
        assert_eq!(
            plan(call(TOOL_ASK, args.clone())).await,
            Err(PlanFault::Unreadable),
            "{args}"
        );
    }
    assert_eq!(
        plan(call(TOOL_ASK, json!({ "text": "\u{202e}\u{200b}" }))).await,
        Err(PlanFault::Unreadable),
        "a question of marks only"
    );
    assert_eq!(
        plan(call(
            TOOL_ASK,
            json!({ "text": "Se\u{202e}nd?", "choices": ["y\u{200b}es"] })
        ))
        .await,
        Ok(ModelOutput::Ask {
            text: "Send?".into(),
            choices: vec!["yes".into()]
        }),
        "marks are not shown"
    );
    assert_eq!(
        plan(call(
            TOOL_ASK,
            json!({ "text": "Q".repeat(400), "choices": ["a"] })
        ))
        .await,
        Ok(ModelOutput::Ask {
            text: "Q".repeat(400),
            choices: vec!["a".into()]
        })
    );
}

#[test]
fn a_tool_name_with_a_homoglyph_is_not_a_name_at_all() {
    // The wire type refuses it before the planner could compare it: a Cyrillic letter is not in
    // the grammar of a tool name.
    assert!(porter_infer::ToolName::parse("org.quire.Mail-mail.thr\u{0435}ad.read").is_err());
}

#[tokio::test]
async fn made_up_and_wrong_typed_calls_are_unread_with_a_typed_fault_not_run() {
    use docket_core::{ArgsFault, ReplyFault, TargetFault};
    let thread = json!({ "app": "org.quire.Mail", "kind": "mail.thread", "key": "t1" });
    let target = |fault| ReplyFault::Args(ArgsFault::Target(fault));
    for (name, args, fault) in [
        (
            "org.quire.Mail-mail.thread.nuke",
            json!({ "target": thread }),
            ReplyFault::NoSuchTool("org.quire.Mail-mail.thread.nuke".into()),
        ),
        (READ, json!({ "target": 5 }), target(TargetFault::Malformed)),
        (READ, json!({}), target(TargetFault::Missing)),
        (
            READ,
            json!({ "target": thread, "admin": true }),
            ReplyFault::Args(ArgsFault::Unknown("admin".into())),
        ),
        (
            READ,
            json!([1, 2]),
            ReplyFault::Args(ArgsFault::NotAnObject),
        ),
        (
            READ,
            json!("target"),
            ReplyFault::Args(ArgsFault::NotAnObject),
        ),
    ] {
        assert_eq!(
            plan(call(name, args.clone())).await,
            Ok(ModelOutput::Unread(fault)),
            "{name} {args}"
        );
    }
}
