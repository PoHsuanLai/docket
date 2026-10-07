//! A reviewer's raw reply: never an allow but for the exact shapes.
#![no_main]

use action_review::parse_verdict;
use docket_core::{Stage, VerdictKind};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: (u8, &str)| {
    let (pick, raw) = input;
    let stage = match pick % 3 {
        0 => Stage::Quick,
        1 => Stage::Deliberate,
        _ => Stage::SecondOpinion,
    };
    let allowed = parse_verdict(raw, stage)
        .map(|v| v.kind() == VerdictKind::Allow)
        .unwrap_or(false);
    if !allowed {
        return;
    }
    match stage {
        Stage::Quick => assert_eq!(raw.trim(), "pass"),
        Stage::Deliberate | Stage::SecondOpinion => {
            let value: serde_json::Value = serde_json::from_str(raw.trim()).expect("JSON");
            let object = value.as_object().expect("an object");
            assert_eq!(object.len(), 3);
            assert_eq!(object["verdict"], "allow");
            // A key written twice would read as its last value: the raw text names each once.
            for key in ["verdict", "code", "reason"] {
                assert_eq!(raw.matches(&format!("\"{key}\"")).count(), 1);
            }
        }
    }
});
