//! The bodies of org.quire.Companion1 from arbitrary text: what reads, reads back the same.
#![no_main]

use companion_wire::{AnswerWire, AskWire};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    if let Ok(answer) = serde_json::from_str::<AnswerWire>(text) {
        let again = serde_json::to_string(&answer).expect("writes");
        assert_eq!(
            serde_json::from_str::<AnswerWire>(&again).expect("reads"),
            answer
        );
    }
    if let Ok(ask) = serde_json::from_str::<AskWire>(text) {
        let again = serde_json::to_string(&ask).expect("writes");
        assert_eq!(serde_json::from_str::<AskWire>(&again).expect("reads"), ask);
    }
});
