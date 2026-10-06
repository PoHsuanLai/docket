//! The footer line, the route log over scripted events, and the refusal text.

use companion_wire::{
    Reached, RouteLog, RouteNote, WhyWord, declined_text, footer_line, model_name, provider_name,
};
use porter_core::{AccountId, Locality, ModelId};
use porter_infer::{
    Declined, DeclinedBecause, Door, InferEvent, InferRefusal, ModelRef, ProviderId, ServedBy,
    StageNote, StageRole, Why,
};

fn model(id: &str) -> ModelRef {
    ModelRef {
        account: AccountId::parse("acct").expect("account"),
        model: ModelId::parse(id).expect("model"),
    }
}

fn served(id: &str, locality: Locality) -> ServedBy {
    let m = model(id);
    ServedBy {
        account: m.account,
        model: m.model,
        locality,
    }
}

fn cloud() -> Locality {
    Locality::Cloud { region: None }
}

fn via(provider: &str, door: Door) -> Why {
    Why::Reached {
        provider: ProviderId(provider.into()),
        door,
    }
}

fn note(stage: StageRole, id: &str, why: Vec<WhyWord>, reached: Option<(&str, Door)>) -> RouteNote {
    RouteNote {
        stage,
        served: served(id, cloud()),
        why,
        reached: reached.map(|(p, door)| Reached {
            provider: ProviderId(p.into()),
            door,
        }),
    }
}

#[test]
fn the_footer_line_table() {
    let table: Vec<(Vec<RouteNote>, &str)> = vec![
        (vec![], ""),
        (
            vec![note(StageRole::Answer, "gemma-4", vec![], None)],
            "Answered by Gemma 4",
        ),
        (
            vec![note(
                StageRole::Answer,
                "claude-haiku-4-5",
                vec![WhyWord::Warm],
                Some(("openrouter", Door::Gateway)),
            )],
            "Answered by Claude Haiku 4.5 via OpenRouter (already loaded)",
        ),
        (
            vec![note(
                StageRole::Answer,
                "claude-haiku-4-5",
                vec![],
                Some(("openrouter", Door::Gateway)),
            )],
            "Answered by Claude Haiku 4.5 via OpenRouter",
        ),
        (
            vec![
                note(StageRole::Hear, "whisper", vec![], None),
                note(
                    StageRole::Answer,
                    "kimi-k2.6",
                    vec![WhyWord::Named],
                    Some(("openrouter", Door::Gateway)),
                ),
            ],
            "Heard by Whisper · Answered by Kimi K2.6 via OpenRouter (you chose it)",
        ),
        (
            vec![note(
                StageRole::Answer,
                "gemma-4",
                vec![
                    WhyWord::Evicted {
                        model: model("qwen-3"),
                    },
                    WhyWord::Smallest,
                ],
                None,
            )],
            "Answered by Gemma 4 (unloaded Qwen 3, quickest to load)",
        ),
        (
            vec![note(
                StageRole::Answer,
                "gpt-5",
                vec![WhyWord::FallbackFrom {
                    model: model("kimi-k2.6"),
                }],
                Some(("openai", Door::Direct)),
            )],
            "Answered by Gpt 5 via OpenAI (Kimi K2.6 could not answer)",
        ),
        (
            vec![
                note(StageRole::Describe, "llava", vec![WhyWord::OnlyOne], None),
                note(StageRole::Speak, "kokoro", vec![], None),
            ],
            "Described by Llava (the only one that fits) · Spoken by Kokoro",
        ),
    ];
    for (notes, line) in table {
        assert_eq!(footer_line(&notes), line, "{notes:?}");
    }
}

#[test]
fn names_read_well() {
    assert_eq!(model_name("claude-haiku-4-5"), "Claude Haiku 4.5");
    assert_eq!(model_name("anthropic/claude-haiku-4-5"), "Claude Haiku 4.5");
    assert_eq!(model_name("kimi-k2.6"), "Kimi K2.6");
    assert_eq!(
        provider_name(&ProviderId("openrouter".into())),
        "OpenRouter"
    );
    assert_eq!(provider_name(&ProviderId("some-host".into())), "Some Host");
}

fn stage(role: StageRole, id: &str, locality: Locality, why: Why) -> InferEvent {
    InferEvent::Stage(StageNote {
        role,
        served: served(id, locality),
        why,
    })
}

#[test]
fn the_log_folds_why_door_and_stage_events() {
    let mut log = RouteLog::default();
    let events = [
        InferEvent::Why(Why::Warm),
        InferEvent::Why(via("openrouter", Door::Gateway)),
        InferEvent::Routed(served("claude-haiku-4-5", cloud())),
        stage(StageRole::Answer, "claude-haiku-4-5", cloud(), Why::Warm),
    ];
    events.iter().for_each(|e| log.event(e));
    let notes = log.notes();
    assert_eq!(
        footer_line(&notes),
        "Answered by Claude Haiku 4.5 via OpenRouter (already loaded)"
    );
}

#[test]
fn reasons_off_leave_the_model_and_the_door_and_an_eviction_stays() {
    // `show_reason` off: no Why event, so the Stage's own `why` is not shown; its door is.
    let mut log = RouteLog::default();
    log.event(&stage(
        StageRole::Answer,
        "kimi-k2.6",
        cloud(),
        via("openrouter", Door::Gateway),
    ));
    assert_eq!(
        footer_line(&log.notes()),
        "Answered by Kimi K2.6 via OpenRouter"
    );
    let mut log = RouteLog::default();
    log.event(&InferEvent::Why(Why::Evicted {
        model: model("qwen-3"),
    }));
    log.event(&stage(
        StageRole::Answer,
        "gemma-4",
        Locality::OnDevice,
        Why::Smallest,
    ));
    assert_eq!(
        footer_line(&log.notes()),
        "Answered by Gemma 4 (unloaded Qwen 3)"
    );
}

#[test]
fn a_pipeline_gives_one_note_per_stage_and_a_bare_routed_one_answer() {
    let mut log = RouteLog::default();
    [
        InferEvent::Routed(served("whisper", Locality::OnDevice)),
        stage(StageRole::Hear, "whisper", Locality::OnDevice, Why::Named),
        InferEvent::Why(Why::Nearest),
        InferEvent::Routed(served("gemma-4", Locality::OnDevice)),
        stage(
            StageRole::Answer,
            "gemma-4",
            Locality::OnDevice,
            Why::Nearest,
        ),
    ]
    .iter()
    .for_each(|e| log.event(e));
    assert_eq!(
        footer_line(&log.notes()),
        "Heard by Whisper · Answered by Gemma 4 (closest to you)"
    );
    let mut log = RouteLog::default();
    log.event(&InferEvent::Why(Why::OnlyOne));
    log.event(&InferEvent::Routed(served("gemma-4", Locality::OnDevice)));
    assert_eq!(
        footer_line(&log.notes()),
        "Answered by Gemma 4 (the only one that fits)"
    );
}

#[test]
fn route_notes_round_trip_as_json() {
    let notes = vec![note(
        StageRole::Answer,
        "claude-haiku-4-5",
        vec![WhyWord::Warm],
        Some(("openrouter", Door::Gateway)),
    )];
    let text = serde_json::to_string(&notes).expect("json");
    let back: Vec<RouteNote> = serde_json::from_str(&text).expect("back");
    assert_eq!(back, notes);
}

#[test]
fn a_declined_model_says_which_and_why() {
    let table = [
        (
            DeclinedBecause::NotInstalled,
            "Kimi K2.6 cannot answer: it is not installed on this computer.",
        ),
        (
            DeclinedBecause::NoRoom,
            "Kimi K2.6 cannot answer: it does not fit in memory.",
        ),
        (
            DeclinedBecause::Blocked {
                refusal: InferRefusal::OverBudget,
            },
            "Kimi K2.6 cannot answer: its spend cap is reached.",
        ),
        (
            DeclinedBecause::NotListed,
            "Kimi K2.6 cannot answer: no account offers it for this request.",
        ),
        (
            DeclinedBecause::Unavailable,
            "Kimi K2.6 cannot answer: it cannot run right now.",
        ),
    ];
    for (because, text) in table {
        let declined = Declined {
            model: model("kimi-k2.6"),
            because,
        };
        assert_eq!(declined_text(&declined), text);
    }
}
