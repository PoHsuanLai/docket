//! The options an app offered for a dynamic parameter (`ParamType::Dynamic`), carried in as data:
//! the caller asks the router (`Suggest`) and hands the answers over, so this crate reaches no
//! bus. A parameter with a few options is offered to the model as a list of exactly those keys.
//! It is advice and not a rule: a key outside the list is still read as the text it is, since an
//! app may have opened since the list was asked.

use crate::catalogue::ENUM_LIMIT;
use docket_core::{ActionRef, OneOf, ParamName};

/// The longest key offered.
const KEY_LIMIT: usize = 128;

/// Whether `key` is plain enough to write into a schema: an id, never sentences.
fn plain(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= KEY_LIMIT
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// The keys an app offered for one parameter of one of its actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamChoices {
    action: ActionRef,
    param: ParamName,
    keys: Vec<String>,
}

impl ParamChoices {
    /// The choices for `param` of `action`, each key once in the order given. `None` when there
    /// are none to offer, or more than a model should be handed as a list (the parameter then
    /// stays free text). A key that is not plain is left out.
    pub fn new(action: ActionRef, param: ParamName, keys: Vec<String>) -> Option<Self> {
        let mut kept: Vec<String> = Vec::new();
        for key in keys.into_iter().filter(|k| plain(k)) {
            if !kept.contains(&key) {
                kept.push(key);
            }
        }
        (1..=ENUM_LIMIT).contains(&kept.len()).then_some(Self {
            action,
            param,
            keys: kept,
        })
    }

    /// The list the model's schema narrows the parameter to.
    pub(crate) fn one_of(&self) -> OneOf {
        OneOf {
            param: self.param.clone(),
            values: self.keys.clone(),
        }
    }

    /// Whether these are the choices of `action`.
    pub(crate) fn is_of(&self, action: &ActionRef) -> bool {
        &self.action == action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prov::ActionName;
    use serde_json::json;

    fn choices(keys: &[&str]) -> Option<ParamChoices> {
        ParamChoices::new(
            ActionRef {
                app: porter_core::AppName::parse("org.quire.Cua").expect("app"),
                name: ActionName::parse("cua.run.start").expect("action"),
            },
            ParamName::parse("app").expect("param"),
            keys.iter().map(|k| (*k).to_owned()).collect(),
        )
    }

    fn narrowed(offered: Option<ParamChoices>) -> serde_json::Value {
        let mut schema = json!({ "properties": { "app": {
            "type": "string",
            "description": "App",
        } } });
        if let Some(offered) = offered {
            offered.one_of().narrow(&mut schema);
        }
        schema["properties"]["app"].clone()
    }

    #[test]
    fn answers_become_an_enum_and_no_answer_stays_a_string() {
        let many: Vec<String> = (0..=ENUM_LIMIT).map(|n| format!("app.n{n}")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        let plain_string = json!({ "type": "string", "description": "App" });
        let cases = [
            (
                choices(&[
                    "org.example.Draw",
                    "app.zen_browser.zen",
                    "org.example.Draw",
                ]),
                json!({
                    "type": "string",
                    "enum": ["org.example.Draw", "app.zen_browser.zen"],
                    "description": "App",
                }),
            ),
            (choices(&[]), plain_string.clone()),
            (choices(&many), plain_string.clone()),
            (choices(&["two words", "ok\nnewline"]), plain_string),
        ];
        for (offered, expected) in cases {
            assert_eq!(narrowed(offered), expected);
        }
    }
}
