//! The bodies of `org.quire.Companion1` read from damaged text: no panic, and anything that does
//! read is stable (written and read again it is the same value).

use companion_wire::{
    AnswerBody, AnswerPhase, AnswerWire, AskWire, FooterWire, NeedsYou, RefusalWire,
};
use docket_core::{ContextKeep, Keep, Reveal};
use proptest::prelude::*;
use prov::TaskId;
use serde::{Serialize, de::DeserializeOwned};

fn steady<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(text: &str) {
    if let Ok(value) = serde_json::from_str::<T>(text) {
        let again = serde_json::to_string(&value).expect("a value that read writes");
        let back: T = serde_json::from_str(&again).expect("what was written reads");
        assert_eq!(back, value, "{text}");
    }
}

fn answer(phase: AnswerPhase, body: AnswerBody) -> String {
    let wire = AnswerWire {
        task: TaskId::parse("t-1").expect("task"),
        phase,
        body,
        footer: FooterWire {
            served: vec![],
            sources: vec![],
            keep: ContextKeep {
                query: Keep::Kept,
                results: Keep::Dropped,
                selection: Keep::Kept,
                window: Keep::Kept,
            },
        },
    };
    serde_json::to_string(&wire).expect("json")
}

fn seeds() -> Vec<String> {
    vec![
        answer(
            AnswerPhase::Done,
            AnswerBody::Text {
                lines: vec![Reveal::Plain("hello".into())],
            },
        ),
        answer(
            AnswerPhase::NeedsYou(NeedsYou::Question {
                text: "Which?".into(),
                choices: vec!["a".into(), "b".into()],
            }),
            AnswerBody::Text { lines: vec![] },
        ),
        answer(
            AnswerPhase::Failed,
            AnswerBody::Refused(RefusalWire::Failed("no".into())),
        ),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn damaged_bodies_never_panic_and_stay_stable(
        seed in prop::sample::select(seeds()),
        edits in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 0..4),
        cut in any::<prop::sample::Index>(),
        keep_cut in any::<bool>(),
    ) {
        let mut bytes = seed.into_bytes();
        for (at, byte) in edits {
            let i = at.index(bytes.len());
            bytes[i] = byte;
        }
        if keep_cut {
            bytes.truncate(cut.index(bytes.len() + 1));
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        steady::<AnswerWire>(&text);
        steady::<AskWire>(&text);
        steady::<Reveal<String>>(&text);
    }

    #[test]
    fn arbitrary_text_is_never_a_panic(text in any::<String>()) {
        steady::<AnswerWire>(&text);
        steady::<AskWire>(&text);
    }
}
