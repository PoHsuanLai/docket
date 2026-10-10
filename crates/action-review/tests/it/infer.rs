//! `InferReviewer` over a scripted model: the stage's own model answers, every failure is an
//! error and never an allow, and the whole cascade only tightens (QUESTIONS D1: a Quick flag
//! still asks even if Deliberate allows).

use crate::support::*;

use action_review::*;
use docket_core::*;
use porter_core::consent::Usage;
use porter_core::{Tier, Tokens};
use porter_infer::{
    ChatReply, InferEvent, Knob, MessagePart, ModelError, Reasoning, ReplyShape, StopReason,
    ToolChoice,
};

#[test]
fn quick_asks_its_own_model_for_one_token() {
    let (quick, deliberate, second) = (Scripted::saying("q", "pass"), silent(), silent());
    let got = block_on(reviewer(&quick, &deliberate, &second).review(Stage::Quick, &request()));
    assert_eq!(got, Ok(ReviewVerdict::Allow));
    assert_eq!(
        (quick.calls(), deliberate.calls(), second.calls()),
        (1, 0, 0)
    );
    let sent = quick.last();
    assert_eq!(
        sent.shape,
        ReplyShape::Choice(vec!["pass".into(), "flag".into()])
    );
    assert_eq!(sent.tier, Tier::Fast);
    assert_eq!(sent.usage, Usage::Interactive);
    assert!(sent.tools.is_empty());
    assert_eq!(sent.control.tool_choice, ToolChoice::Never);
    assert!(matches!(sent.control.max_output, Knob::Set(Tokens(n)) if n <= 8));
    assert_eq!(sent.class, REVIEW_CLASS);
}

/// The policy point for the person's words: every stage's request carries `Prompt`, and the
/// proposed AI policy pins that class to this computer, so no cloud or network model is admitted
/// however the models are configured.
#[test]
fn every_reviewer_request_carries_the_prompt_class_pinned_on_device() {
    use porter_core::{DataClass, Locality};
    use porter_infer::{Floor, Policy};

    assert_eq!(REVIEW_CLASS, DataClass::Prompt);
    let policy = Policy::proposed();
    let floor = policy.floor(REVIEW_CLASS);
    assert_eq!(floor, Floor::OnDevice, "the proposed floor of a prompt");
    assert!(floor.admits(&Locality::OnDevice));
    assert!(!floor.admits(&Locality::LocalNetwork));
    assert!(!floor.admits(&Locality::Cloud { region: None }));
    // `AppOwn`, which the first version used, may go anywhere: the class matters.
    assert_eq!(policy.floor(DataClass::AppOwn), Floor::Anywhere);

    for (stage, model) in [
        (Stage::Quick, 0),
        (Stage::Deliberate, 1),
        (Stage::SecondOpinion, 2),
    ] {
        let models = [
            Scripted::saying("q", "pass"),
            Scripted::saying("d", ALLOW),
            Scripted::saying("s", ALLOW),
        ];
        block_on(reviewer(&models[0], &models[1], &models[2]).review(stage, &request()))
            .expect("verdict");
        let sent = models[model].last();
        assert_eq!(sent.class, DataClass::Prompt, "{stage:?}");
        assert_eq!(policy.floor(sent.class), Floor::OnDevice, "{stage:?}");
    }
}

#[test]
fn the_larger_stages_ask_for_a_record_from_their_own_models() {
    let (quick, deliberate, second) = (
        silent(),
        Scripted::saying("d", ALLOW),
        Scripted::saying("s", DENY),
    );
    let r = reviewer(&quick, &deliberate, &second);
    assert_eq!(
        block_on(r.review(Stage::Deliberate, &request())),
        Ok(ReviewVerdict::Allow)
    );
    assert!(matches!(
        block_on(r.review(Stage::SecondOpinion, &request())),
        Ok(ReviewVerdict::Deny { .. })
    ));
    assert_eq!(
        (quick.calls(), deliberate.calls(), second.calls()),
        (0, 1, 1)
    );
    let sent = deliberate.last();
    assert!(matches!(&sent.shape, ReplyShape::Json(schema) if schema.contains("outside_request")));
    assert_eq!(sent.tier, Tier::Balanced);
    assert_eq!(second.last().tier, Tier::Best);
}

#[test]
fn the_messages_are_the_rendered_prompt() {
    let (quick, deliberate, second) = (Scripted::saying("q", "pass"), silent(), silent());
    block_on(reviewer(&quick, &deliberate, &second).review(Stage::Quick, &request()))
        .expect("verdict");
    let prompt = render(&request(), Stage::Quick);
    let texts: Vec<String> = quick
        .last()
        .messages
        .iter()
        .flat_map(|m| m.parts.iter())
        .filter_map(|p| match p {
            MessagePart::Text(t) => Some(t.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec![prompt.system, prompt.user]);
}

#[test]
fn a_failure_is_an_error_and_never_an_allow() {
    let rows: [(&str, Result<ChatReply, ModelError>, ReviewError); 11] = [
        (
            "unreachable",
            Err(ModelError::Unreachable),
            ReviewError::Unavailable,
        ),
        (
            "not ready",
            Err(ModelError::NotReady),
            ReviewError::Unavailable,
        ),
        (
            "rate limited",
            Err(ModelError::RateLimited(5)),
            ReviewError::Unavailable,
        ),
        (
            "unauthorized",
            Err(ModelError::Unauthorized),
            ReviewError::Unavailable,
        ),
        (
            "needs payment",
            Err(ModelError::PaymentRequired),
            ReviewError::Unavailable,
        ),
        (
            "sign-in refused",
            Err(ModelError::SignInRefused),
            ReviewError::Unavailable,
        ),
        (
            "refused",
            Err(ModelError::Refused),
            ReviewError::Unavailable,
        ),
        (
            "overflow",
            Err(ModelError::ContextOverflow),
            ReviewError::Unavailable,
        ),
        (
            "unparseable",
            Err(ModelError::Unparseable),
            ReviewError::Unparseable,
        ),
        (
            "unreadable",
            Err(ModelError::Unreadable),
            ReviewError::Unparseable,
        ),
        (
            "cut by the limit",
            Ok(reply(&silent().card, "pass", StopReason::MaxTokens)),
            ReviewError::OutOfRoom,
        ),
    ];
    for (name, scripted, want) in rows {
        let quick = Scripted::new("q", vec![scripted]);
        let got = block_on(reviewer(&quick, &silent(), &silent()).review(Stage::Quick, &request()));
        assert_eq!(got, Err(want), "{name}");
    }
}

#[test]
fn a_filtered_or_tool_reply_is_not_a_verdict() {
    for stop in [StopReason::ContentFilter, StopReason::ToolUse] {
        let quick = Scripted::new("q", vec![Ok(reply(&silent().card, "pass", stop))]);
        let got = block_on(reviewer(&quick, &silent(), &silent()).review(Stage::Quick, &request()));
        assert_eq!(got, Err(ReviewError::Unparseable), "{stop:?}");
    }
}

#[test]
fn a_reply_outside_the_shape_is_not_an_allow() {
    for text in ["", "yes", "pass it", ALLOW] {
        let quick = Scripted::saying("q", text);
        let got = block_on(reviewer(&quick, &silent(), &silent()).review(Stage::Quick, &request()));
        assert!(!matches!(got, Ok(ReviewVerdict::Allow)), "{text:?}");
    }
    for text in ["", "allow", "pass", "{}", r#"{"verdict":"allow"}"#] {
        let deliberate = Scripted::saying("d", text);
        let got = block_on(
            reviewer(&silent(), &deliberate, &silent()).review(Stage::Deliberate, &request()),
        );
        assert!(got.is_err(), "{text:?}");
    }
}

#[test]
fn a_stage_with_no_time_fails_before_asking() {
    let quick = Scripted::saying("q", "pass");
    let mut r = reviewer(&quick, &silent(), &silent());
    r.timeouts.quick = Millis(0);
    assert_eq!(
        block_on(r.review(Stage::Quick, &request())),
        Err(ReviewError::Timeout)
    );
    assert_eq!(quick.calls(), 0);
}

/// Runs the cascade the way the router does: plan, run each stage, escalate after a Quick flag,
/// then tighten.
fn cascade(ruling: &Ruling, impact: Impact, r: &InferReviewer<Scripted>) -> Gate {
    let req = request();
    let mut planned = plan(ruling, impact);
    let mut verdicts = Vec::new();
    let mut at = 0;
    while at < planned.len() {
        let stage = planned[at];
        let verdict = block_on(r.review(stage, &req));
        if stage == Stage::Quick {
            planned = escalate(&planned, &verdict);
        }
        verdicts.push((stage, verdict));
        at += 1;
    }
    tighten(ruling, &planned, &verdicts)
}

#[test]
fn a_quick_flag_still_asks_even_if_deliberate_allows() {
    let (quick, deliberate) = (Scripted::saying("q", "flag"), Scripted::saying("d", ALLOW));
    let judged = Ruling::AllowJudged(vec![]);
    let gate = cascade(
        &judged,
        Impact::Low,
        &reviewer(&quick, &deliberate, &silent()),
    );
    assert_eq!(gate, Gate::Confirm);
    assert_eq!(
        deliberate.calls(),
        1,
        "the flag escalated to the larger model"
    );
}

#[test]
fn a_low_impact_call_runs_only_when_quick_passes() {
    let quick = Scripted::saying("q", "pass");
    let gate = cascade(
        &Ruling::AllowJudged(vec![]),
        Impact::Low,
        &reviewer(&quick, &silent(), &silent()),
    );
    assert_eq!(gate, Gate::Run);
}

#[test]
fn a_high_impact_call_needs_all_three_and_one_deny_refuses() {
    let all_allow = reviewer(
        &Scripted::saying("q", "pass"),
        &Scripted::saying("d", ALLOW),
        &Scripted::saying("s", ALLOW),
    );
    assert_eq!(
        cascade(&Ruling::AllowJudged(vec![]), Impact::High, &all_allow),
        Gate::Run
    );
    let second_denies = reviewer(
        &Scripted::saying("q", "pass"),
        &Scripted::saying("d", ALLOW),
        &Scripted::saying("s", DENY),
    );
    assert_eq!(
        cascade(&Ruling::AllowJudged(vec![]), Impact::High, &second_denies),
        Gate::Refuse(DenyCode::NotAllowed)
    );
    let second_asks = reviewer(
        &Scripted::saying("q", "pass"),
        &Scripted::saying("d", ALLOW),
        &Scripted::saying("s", ASK),
    );
    assert_eq!(
        cascade(&Ruling::AllowJudged(vec![]), Impact::High, &second_asks),
        Gate::Confirm
    );
    let second_down = reviewer(
        &Scripted::saying("q", "pass"),
        &Scripted::saying("d", ALLOW),
        &silent(),
    );
    assert_eq!(
        cascade(&Ruling::AllowJudged(vec![]), Impact::High, &second_down),
        Gate::Confirm
    );
}

#[test]
fn a_hijacked_reviewer_never_loosens_ask_or_deny() {
    let always = || {
        reviewer(
            &Scripted::saying("q", "pass"),
            &Scripted::saying("d", ALLOW),
            &Scripted::saying("s", ALLOW),
        )
    };
    let ask = Ruling::Ask(vec![AskReason::RuleOfTwo]);
    let deny = Ruling::Deny(vec![PolicyId("hidden-from-agents".into())]);
    for impact in [Impact::Low, Impact::High] {
        let r = always();
        assert_eq!(cascade(&ask, impact, &r), Gate::Confirm);
        assert_eq!(
            cascade(&deny, impact, &r),
            Gate::Refuse(DenyCode::NotAllowed)
        );
        assert_eq!(
            r.quick.calls() + r.deliberate.calls() + r.second.calls(),
            0,
            "no stage ran"
        );
    }
}

#[test]
fn the_schema_the_model_is_given_names_the_codes_the_parser_reads() {
    use model_provider::Shape;
    let Shape::Record(fields) = verdict_shape(Stage::Deliberate) else {
        panic!("a record")
    };
    let code = fields
        .iter()
        .find(|f| f.name.as_str() == "code")
        .expect("code field");
    let Shape::Choice(codes) = &code.shape else {
        panic!("a choice")
    };
    let (deliberate, second) = (Scripted::saying("d", ALLOW), Scripted::saying("s", ALLOW));
    let r = reviewer(&silent(), &deliberate, &second);
    block_on(r.review(Stage::Deliberate, &request())).expect("verdict");
    block_on(r.review(Stage::SecondOpinion, &request())).expect("verdict");
    let (ReplyShape::Json(schema), ReplyShape::Json(again)) =
        (deliberate.last().shape, second.last().shape)
    else {
        panic!("json shapes")
    };
    assert_eq!(schema, again);
    for c in codes {
        assert!(
            schema.contains(&format!("\"{}\"", c.0)),
            "{} missing from {schema}",
            c.0
        );
    }
    assert_eq!(schema.matches("\"enum\"").count(), 2);
    // Every code in the schema parses back, so the shape the model is held to is the shape read.
    for c in codes {
        let verdict =
            if ["within_request", "covered_by_task_policy", "routine"].contains(&c.0.as_str()) {
                "allow"
            } else {
                "ask"
            };
        let raw = format!(r#"{{"verdict":"{verdict}","code":"{}","reason":"r"}}"#, c.0);
        assert!(parse_verdict(&raw, Stage::Deliberate).is_ok(), "{raw}");
    }
}

#[test]
fn no_stage_asks_for_reasoning() {
    for (stage, _) in [
        (Stage::Quick, 0),
        (Stage::Deliberate, 1),
        (Stage::SecondOpinion, 2),
    ] {
        let models = [
            Scripted::saying("q", "pass"),
            Scripted::saying("d", ALLOW),
            Scripted::saying("s", ALLOW),
        ];
        block_on(reviewer(&models[0], &models[1], &models[2]).review(stage, &request()))
            .expect("verdict");
        let sent = models
            .iter()
            .find(|m| m.calls() == 1)
            .expect("one model asked")
            .last();
        assert_eq!(sent.control.reasoning, Reasoning::Off, "{stage:?}");
    }
}

#[test]
fn a_reply_that_is_only_thought_has_its_own_cause() {
    let model = Scripted::new("d", vec![]);
    let mut thought = reply(&model.card, "", StopReason::MaxTokens);
    thought.thought = Some("let me think about the request".into());
    model.push(Ok(thought));
    let got =
        block_on(reviewer(&silent(), &model, &silent()).review(Stage::Deliberate, &request()));
    assert_eq!(got, Err(ReviewError::OnlyThought));
}

#[test]
fn a_failed_turn_that_only_streamed_thought_is_only_thought() {
    let thinking = vec![InferEvent::ThoughtDelta("hmm".into())];
    let model = Scripted::new("d", vec![Err(ModelError::Unparseable)]).sending(thinking.clone());
    let got =
        block_on(reviewer(&silent(), &model, &silent()).review(Stage::Deliberate, &request()));
    assert_eq!(got, Err(ReviewError::OnlyThought));
    let mut mixed = thinking;
    mixed.push(InferEvent::TextDelta("{".into()));
    let model = Scripted::new("d", vec![Err(ModelError::Unparseable)]).sending(mixed);
    let got =
        block_on(reviewer(&silent(), &model, &silent()).review(Stage::Deliberate, &request()));
    assert_eq!(got, Err(ReviewError::Unparseable));
}
