//! The watched-agent shapes: an app's conversation id is one plain word, a seen command line is
//! bounded and does not print its words.

use companion_model::{AppSessionId, SeenCommand, SeenCommandFault};
use serde::{Serialize, de::DeserializeOwned};

fn round<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: &T) -> String {
    let json = serde_json::to_string(value).expect("json");
    let back: T = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{json}: {e}"));
    assert_eq!(&back, value, "{json}");
    json
}

#[test]
fn an_app_conversation_id_is_one_plain_word_and_a_seen_command_line_is_bounded() {
    let long_id = "a".repeat(129);
    let id_cases: &[(&str, bool)] = &[
        ("0a1b-2c3d", true),
        ("a_b.c:d", true),
        ("", false),
        ("-x", false),
        ("a b", false),
        ("a/b", false),
        (long_id.as_str(), false),
    ];
    for (text, ok) in id_cases {
        assert_eq!(AppSessionId::parse(text).is_ok(), *ok, "{text}");
    }
    // The wire form is the plain string, and a bad one does not read.
    assert_eq!(round(&AppSessionId::parse("ab-1").expect("id")), "\"ab-1\"");
    assert!(serde_json::from_str::<AppSessionId>("\"-x\"").is_err());

    let words = |n: usize, each: usize| vec!["w".repeat(each); n];
    let line_cases: &[(Vec<String>, Result<(), SeenCommandFault>)] = &[
        (words(1, 3), Ok(())),
        (words(256, 1), Ok(())),
        (vec![], Err(SeenCommandFault::Empty)),
        (words(257, 1), Err(SeenCommandFault::TooManyWords)),
        (words(2, 40_000), Err(SeenCommandFault::TooLong)),
        (vec!["a\0b".to_owned()], Err(SeenCommandFault::Nul)),
    ];
    for (given, want) in line_cases {
        assert_eq!(SeenCommand::parse(given.clone()).map(drop), *want);
    }
    // Argv can hold secrets: the debug form says only how many words there are.
    let seen =
        SeenCommand::parse(vec!["tool".to_owned(), "--key=hunter2".to_owned()]).expect("seen");
    assert_eq!(format!("{seen:?}"), "SeenCommand(2 words)");
    assert_eq!(seen.tokens().len(), 2);
}
