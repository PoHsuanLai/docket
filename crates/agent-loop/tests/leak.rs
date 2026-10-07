//! `leaked_call`: a call written as words is told from words about calls.

use agent_loop::{LeakForm, leaked_call};
use proptest::prelude::*;

#[test]
fn the_templates_are_told_apart() {
    let table = [
        (
            "hermes",
            "<tool_call>\n{\"name\": \"f\", \"arguments\": {}}\n</tool_call>",
            Some(LeakForm::Hermes),
        ),
        (
            "qwen xml",
            "<tool_call>\n<function=f>\n<parameter=a>\n1\n</parameter>\n</function>\n</tool_call>",
            Some(LeakForm::QwenXml),
        ),
        (
            "mistral",
            "[TOOL_CALLS] [{\"name\": \"f\"}]",
            Some(LeakForm::Mistral),
        ),
        ("llama", "<|python_tag|>f()", Some(LeakForm::LlamaTag)),
        (
            "other",
            "<function_call>{}</function_call>",
            Some(LeakForm::OtherTag),
        ),
        (
            "words",
            "I called the function and the tool is ready.",
            None,
        ),
        ("a tag that is not one", "<b>tool</b> <call>", None),
        ("empty", "", None),
    ];
    for (name, text, want) in table {
        assert_eq!(leaked_call(text).map(|l| l.form), want, "{name}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn never_panics_on_any_text(text in any::<String>()) {
        let _ = leaked_call(&text);
    }

    #[test]
    fn text_without_an_opening_bracket_is_never_a_call(text in "[^<\\[\u{ff1c}\u{3008}\u{2039}]*") {
        prop_assert_eq!(leaked_call(&text), None);
    }

    #[test]
    fn dressing_the_marker_does_not_hide_it(
        gaps in prop::collection::vec(prop::sample::select(vec!["", "\u{200b}", "\u{202e}", "\u{feff}", "\u{2060}", "\u{ad}"]), 11),
        upper in any::<bool>(),
        before in "[a-z ]{0,20}",
    ) {
        let marker = "<tool_call>";
        let dressed: String = marker
            .chars()
            .zip(gaps.iter().chain(std::iter::repeat(&"")))
            .map(|(c, g)| format!("{g}{}", if upper { c.to_ascii_uppercase() } else { c }))
            .collect();
        let text = format!("{before}{dressed}{{}}");
        prop_assert!(leaked_call(&text).is_some(), "{:?}", text);
    }
}
