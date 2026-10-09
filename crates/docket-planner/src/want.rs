//! The `want` of a `quire_read`: the schema the model is shown, and the options of a `choice`
//! read as ids. A model that writes an option as words ("Option A") has said what it
//! meant, so the words become the id they name; when they name none, or two options name the
//! same one, it is told which option and what an option looks like.

use docket_core::{CharCount, ChoiceId, ReadFault};
use serde_json::Value as Json;
use std::collections::HashSet;

/// What an option may be, as a pattern: the id grammar.
const OPTION_PATTERN: &str = "^[a-z0-9][a-z0-9_.-]{0,63}$";

/// The longest a text answer may be when the model gives no `max`: a summary or a short reply,
/// not a document. Named in the `want` schema the model is shown.
pub(crate) const DEFAULT_TEXT_MAX: CharCount = CharCount(2000);

/// The most characters of a bad option quoted back to the model.
const QUOTED: usize = 60;

/// The tool's `want` parameter: the closed shapes of an answer, as the reader takes them.
pub(crate) fn want_schema() -> Json {
    serde_json::json!({
        "type": "object",
        "description": "The shape of the answer, as {\"kind\": ..., \"v\": ...}. Kinds: choice (v: list of option ids, each lowercase letters, digits and _, such as \"option_a\"), integer (v: {min, max}), date, datetime, text (v: {max}; v may be left out for the default of 2000 characters), record (v: list of [name, shape]), list (v: {of: shape, max}). Example: {\"kind\": \"choice\", \"v\": [\"yes\", \"no\"]}. To decide something, ask for choice, integer, date or datetime: text, and a record or list holding text, comes back as a handle you cannot read.",
        "properties": {
            "kind": { "enum": ["choice", "integer", "date", "datetime", "text", "record", "list"] },
            "v": {
                "anyOf": [
                    { "type": "array", "items": { "anyOf": [
                        { "type": "string", "pattern": OPTION_PATTERN },
                        { "type": "array" },
                    ] } },
                    { "type": "object" },
                ],
            },
        },
        "required": ["kind"],
    })
}

/// A `text` shape with its `max`: the one given, or the default when `v` is absent, `null` or
/// has no `max`; a `v` that is anything else is told as such.
fn text_with_max(v: Option<&Json>) -> Result<Json, ReadFault> {
    let max = match v {
        None | Some(Json::Null) => DEFAULT_TEXT_MAX.0,
        Some(Json::Object(fields)) => match fields.get("max") {
            None | Some(Json::Null) => DEFAULT_TEXT_MAX.0,
            Some(n) => n
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0)
                .ok_or(ReadFault::WantText)?,
        },
        Some(_) => return Err(ReadFault::WantText),
    };
    Ok(serde_json::json!({ "kind": "text", "v": { "max": max } }))
}

/// `want` made ready to read: every `choice` option (at any depth) written as its id, and every
/// `text` given its `max`; or why one cannot be.
pub(crate) fn settle_want(want: &Json) -> Result<Json, ReadFault> {
    let Some(kind) = want.get("kind").and_then(Json::as_str) else {
        return Ok(want.clone());
    };
    if kind == "text" {
        return text_with_max(want.get("v"));
    }
    let Some(v) = want.get("v") else {
        return Ok(want.clone());
    };
    let v = match (kind, v) {
        ("choice", Json::Array(options)) => Json::Array(option_ids(options)?),
        ("list", Json::Object(list)) => {
            let mut list = list.clone();
            if let Some(of) = list.get("of") {
                let of = settle_want(of)?;
                list.insert("of".to_owned(), of);
            }
            Json::Object(list)
        }
        ("record", Json::Array(fields)) => Json::Array(
            fields
                .iter()
                .map(|field| match field {
                    Json::Array(pair) if pair.len() == 2 => {
                        Ok(Json::Array(vec![pair[0].clone(), settle_want(&pair[1])?]))
                    }
                    other => Ok(other.clone()),
                })
                .collect::<Result<_, ReadFault>>()?,
        ),
        _ => v.clone(),
    };
    Ok(serde_json::json!({ "kind": kind, "v": v }))
}

/// The options as ids. A value that is not a string is left for the parse to refuse.
fn option_ids(options: &[Json]) -> Result<Vec<Json>, ReadFault> {
    let mut seen = HashSet::new();
    options
        .iter()
        .map(|option| {
            let Some(text) = option.as_str() else {
                return Ok(option.clone());
            };
            let id = ChoiceId::slug(text)
                .ok_or_else(|| ReadFault::WantOption(text.chars().take(QUOTED).collect()))?;
            if !seen.insert(id.clone()) {
                return Err(ReadFault::WantClash(id));
            }
            Ok(Json::String(id.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ids(want: Json) -> Result<Json, ReadFault> {
        settle_want(&want)
    }

    #[test]
    fn options_written_as_words_become_the_ids_they_name() {
        let got = ids(json!({ "kind": "choice", "v": ["Option A", "skip"] }));
        assert_eq!(
            got,
            Ok(json!({ "kind": "choice", "v": ["option_a", "skip"] }))
        );
    }

    #[test]
    fn a_choice_inside_a_record_or_a_list_is_read_the_same_way() {
        let got = ids(json!({ "kind": "record", "v": [
            ["kind", { "kind": "choice", "v": ["Not A"] }],
            ["rest", { "kind": "list", "v": { "of": { "kind": "choice", "v": ["A b"] }, "max": 3 } }],
        ] }))
        .expect("ids");
        assert_eq!(got["v"][0][1]["v"], json!(["not_a"]));
        assert_eq!(got["v"][1][1]["v"]["of"]["v"], json!(["a_b"]));
    }

    #[test]
    fn two_options_that_name_one_id_are_refused() {
        let got = ids(
            json!({ "kind": "choice", "v": ["Option A", "option-a!", "option_a"] }),
        );
        let clash = ChoiceId::parse("option_a").expect("id");
        assert_eq!(got, Err(ReadFault::WantClash(clash)));
    }

    #[test]
    fn an_option_with_nothing_to_make_an_id_of_is_named() {
        let got = ids(json!({ "kind": "choice", "v": ["forward", "???"] }));
        assert_eq!(got, Err(ReadFault::WantOption("???".to_owned())));
    }

    #[test]
    fn a_text_without_a_length_is_read_with_the_default() {
        let default = json!({ "kind": "text", "v": { "max": 2000 } });
        assert_eq!(ids(json!({ "kind": "text" })), Ok(default.clone()));
        assert_eq!(ids(json!({ "kind": "text", "v": {} })), Ok(default.clone()));
        assert_eq!(ids(json!({ "kind": "text", "v": null })), Ok(default));
        assert_eq!(
            ids(json!({ "kind": "text", "v": { "max": 300 } })),
            Ok(json!({ "kind": "text", "v": { "max": 300 } }))
        );
    }

    #[test]
    fn a_text_inside_a_record_or_a_list_gets_the_default_too() {
        let got = ids(json!({ "kind": "record", "v": [
            ["summary", { "kind": "text" }],
            ["parts", { "kind": "list", "v": { "of": { "kind": "text", "v": {} }, "max": 3 } }],
        ] }))
        .expect("ids");
        assert_eq!(got["v"][0][1]["v"]["max"], json!(2000));
        assert_eq!(got["v"][1][1]["v"]["of"]["v"]["max"], json!(2000));
    }

    #[test]
    fn a_text_with_a_v_that_is_not_a_length_is_told_so() {
        for v in [
            json!("long"),
            json!(5),
            json!([3]),
            json!({ "max": "big" }),
            json!({ "max": 0 }),
            json!({ "max": -4 }),
        ] {
            assert_eq!(
                ids(json!({ "kind": "text", "v": v })),
                Err(ReadFault::WantText),
                "{v}"
            );
        }
    }

    #[test]
    fn the_schema_names_the_default_text_length() {
        assert!(
            want_schema()
                .to_string()
                .contains("default of 2000 characters")
        );
    }

    #[test]
    fn the_schema_says_which_answers_come_back_readable() {
        let schema = want_schema().to_string();
        assert!(
            schema.contains("comes back as a handle you cannot read"),
            "{schema}"
        );
    }

    #[test]
    fn the_schema_gives_choice_options_the_id_pattern() {
        let schema = want_schema().to_string();
        assert!(
            schema.contains("\"pattern\":\"^[a-z0-9][a-z0-9_.-]{0,63}$\""),
            "{schema}"
        );
    }
}
