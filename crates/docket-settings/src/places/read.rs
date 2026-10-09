//! The lenient reader of the places: anything it does not clearly understand is Off, never On.

use super::model::{
    CloudAccount, CloudAllowed, ModelChoice, OwnComputer, PlaceName, Places, Toggle,
};
use crate::{Fallback, Why};

/// Where the places live in the settings file.
pub const PLACES_PATH: &str = "assistant";

/// The places read, and the rows that fell back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacesLoaded {
    /// The places in force.
    pub places: Places,
    /// Values refused (the row is then Off).
    pub fallbacks: Vec<Fallback>,
}

fn text(t: &toml::Table, key: &str) -> Option<String> {
    t.get(key)?.as_str().map(str::to_owned)
}

fn model(t: &toml::Table) -> Option<ModelChoice> {
    text(t, "model").filter(|m| !m.is_empty()).map(ModelChoice)
}

fn rows<'a>(doc: &'a toml::Table, key: &str) -> Vec<&'a toml::Table> {
    doc.get("assistant")
        .and_then(toml::Value::as_table)
        .and_then(|a| a.get(key))
        .and_then(toml::Value::as_array)
        .map(|rows| rows.iter().filter_map(toml::Value::as_table).collect())
        .unwrap_or_default()
}

fn word<'a, T: Copy>(
    row: &toml::Table,
    key: &str,
    words: &'static [&'static str],
    values: &'a [T],
    off: T,
    fallbacks: &mut Vec<Fallback>,
) -> T {
    let found = row
        .get("allowed")
        .and_then(toml::Value::as_str)
        .and_then(|w| words.iter().position(|x| *x == w));
    found.map(|i| values[i]).unwrap_or_else(|| {
        fallbacks.push(Fallback {
            key: format!("{PLACES_PATH}.{key}.allowed"),
            why: Why::NotAWord { words },
        });
        off
    })
}

/// Reads the places from settings text. Rows without a name are dropped; a missing or unknown
/// `allowed` is Off (and reported); "ask" on a computer of the person's is Off.
pub fn read_places(text_in: &str) -> PlacesLoaded {
    let Ok(doc) = text_in.parse::<toml::Table>() else {
        return PlacesLoaded {
            places: Places::none(),
            fallbacks: Vec::new(),
        };
    };
    let mut fallbacks = Vec::new();
    let this_computer = doc
        .get("assistant")
        .and_then(toml::Value::as_table)
        .and_then(|a| a.get("this_computer"))
        .and_then(toml::Value::as_table)
        .and_then(model);
    let own = rows(&doc, "own_computers")
        .into_iter()
        .filter_map(|row| {
            Some(OwnComputer {
                name: PlaceName(text(row, "name")?),
                allowed: word(
                    row,
                    "own_computers",
                    &["off", "on"],
                    &[Toggle::Off, Toggle::On],
                    Toggle::Off,
                    &mut fallbacks,
                ),
                model: model(row),
            })
        })
        .collect();
    let cloud = rows(&doc, "cloud_accounts")
        .into_iter()
        .filter_map(|row| {
            Some(CloudAccount {
                name: PlaceName(text(row, "name")?),
                provider: text(row, "provider").unwrap_or_default(),
                allowed: word(
                    row,
                    "cloud_accounts",
                    &["off", "on", "ask"],
                    &[
                        CloudAllowed::Off,
                        CloudAllowed::On,
                        CloudAllowed::AskEachTime,
                    ],
                    CloudAllowed::Off,
                    &mut fallbacks,
                ),
                model: model(row),
            })
        })
        .collect();
    PlacesLoaded {
        places: Places {
            this_computer,
            own,
            cloud,
        },
        fallbacks,
    }
}
