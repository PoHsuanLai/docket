//! The stripped review request holds no untrusted text, and the render and parse of the
//! reviewer's prompt wait for their fill.

use action_review::*;
use docket_core::*;
use porter_core::{AppName, Count};
use prov::{ActionName, Effect, Integrity, Label, Source, SpaceId};
use std::collections::{BTreeMap, BTreeSet};

fn request() -> ReviewRequest {
    ReviewRequest {
        space: SpaceId::parse("work").expect("space"),
        strictness: Strictness::Default,
        turns: vec![UserTurn {
            id: TurnId(1),
            text: "forward the receipts".into(),
            at: prov::UnixSeconds(1),
            from: TurnSource::Launcher,
            via: TurnVia::Typed,
        }],
        proposed: ProposedAction {
            app: AppName::parse("org.quire.Mail").expect("app"),
            action: ActionName::parse("mail.message.forward").expect("action"),
            label: LabelText::parse("Forward").expect("label"),
            effect: Effect::Outbound,
            kinds: BTreeSet::new(),
            count: Count(2),
            args: vec![(
                ParamName::parse("body").expect("param"),
                ArgSink::Body,
                ArgView::Untrusted {
                    from: BTreeSet::from([Source::Mail]),
                    size: CharCount(120),
                },
            )],
            lasting: Lasting::No,
        },
        labels: ArgLabels {
            per_arg: BTreeMap::<ParamName, Label>::new(),
            planner: Integrity::Untrusted,
            saw: SessionSaw {
                private: Saw::Seen,
                untrusted: Saw::Seen,
            },
        },
        task_policy: None,
        history: vec![],
    }
}

#[test]
fn an_untrusted_argument_appears_as_its_sources_and_size() {
    let json = serde_json::to_string(&request()).expect("json");
    assert!(
        json.contains(r#""kind":"untrusted""#) && json.contains("120"),
        "{json}"
    );
    let back: ReviewRequest = serde_json::from_str(&json).expect("round trip");
    assert_eq!(back, request());
}

#[test]
fn the_debug_of_a_request_holds_no_words_of_the_person() {
    let text = format!("{:?}", request());
    assert!(!text.contains("forward the receipts"), "{text}");
}

#[test]
#[ignore = "render and parse_verdict are the action-review fill"]
fn render_is_deterministic_and_parse_never_allows_by_failing() {
    let a = render(&request(), Stage::Quick);
    assert_eq!(a, render(&request(), Stage::Quick));
    for junk in [
        "",
        "allow",
        "{\"verdict\":\"allow\"",
        "flag!",
        "PASS please",
    ] {
        let got = parse_verdict(junk, Stage::Deliberate);
        assert!(!matches!(got, Ok(ReviewVerdict::Allow)), "{junk:?}");
    }
    assert_eq!(
        parse_verdict("pass", Stage::Quick),
        Ok(ReviewVerdict::Allow)
    );
}
