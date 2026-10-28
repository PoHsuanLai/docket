//! The key table: every `agent.*` key of design/22 section 3.27, its rule and where its value
//! goes. One row per schema row (`dist/settings/docket.settings.toml`); the tests hold the two
//! tables to each other, so a row without a reader or a reader without a row fails.

use crate::AgentSettings;
use model_provider::Money;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde::de::value::{Error as DeError, StrDeserializer};
use serde::de::{DeserializeOwned, IntoDeserializer};
use std::ops::RangeInclusive;

/// What a key's value may be and how it lands in the settings.
#[derive(Clone)]
pub(crate) enum Rule {
    /// A whole number inside `range`, in the key's unit.
    Number {
        range: RangeInclusive<i64>,
        set: fn(&mut AgentSettings, i64),
    },
    /// One of `words`, read by the target type's serde name; `set` says whether it took.
    Word {
        words: &'static [&'static str],
        set: fn(&mut AgentSettings, &str) -> bool,
    },
}

/// One key of the table.
#[derive(Clone)]
pub(crate) struct Key {
    pub path: &'static str,
    pub rule: Rule,
}

const fn num(
    path: &'static str,
    range: RangeInclusive<i64>,
    set: fn(&mut AgentSettings, i64),
) -> Key {
    Key {
        path,
        rule: Rule::Number { range, set },
    }
}

/// Reads `word` into `slot` by the type's serde name; whether it was one.
fn parse_into<T: DeserializeOwned>(slot: &mut T, word: &str) -> bool {
    let de: StrDeserializer<'_, DeError> = word.into_deserializer();
    match <T as Deserialize<'_>>::deserialize(de) {
        Ok(value) => {
            *slot = value;
            true
        }
        Err(_) => false,
    }
}

/// Keys an older settings file may still hold, read before the table so a new key beside them
/// wins. They are not schema rows.
pub(crate) fn legacy() -> Vec<Key> {
    vec![
        // `ask_more` is Manual, `default` and `trust_more` are Auto: `Mode` reads all three.
        word(
            "agent.strictness",
            &["ask_more", "default", "trust_more"],
            |s, w| parse_into(&mut s.agent.start_mode, w),
        ),
        // The single row of the first permissions release: its word is the start mode.
        word("agent.permissions", &["auto", "manual"], |s, w| {
            parse_into(&mut s.agent.start_mode, w)
        }),
    ]
}

const fn word(
    path: &'static str,
    words: &'static [&'static str],
    set: fn(&mut AgentSettings, &str) -> bool,
) -> Key {
    Key {
        path,
        rule: Rule::Word { words, set },
    }
}

/// The start mode and the ten rows of the two modes.
fn mode_keys() -> Vec<Key> {
    let inside: &[&str] = &["runs", "asks"];
    let other: &[&str] = &["check", "asks"];
    let ask_or_runs: &[&str] = &["asks", "runs"];
    let screen: &[&str] = &["once", "each"];
    vec![
        word("agent.start_mode", &["auto", "manual"], |s, w| {
            parse_into(&mut s.agent.start_mode, w)
        }),
        word("agent.auto.inside", inside, |s, w| {
            parse_into(&mut s.agent.rows.auto.inside, w)
        }),
        word("agent.auto.other", other, |s, w| {
            parse_into(&mut s.agent.rows.auto.other, w)
        }),
        word("agent.auto.send", ask_or_runs, |s, w| {
            parse_into(&mut s.agent.rows.auto.send, w)
        }),
        word("agent.auto.delete", ask_or_runs, |s, w| {
            parse_into(&mut s.agent.rows.auto.delete, w)
        }),
        word("agent.auto.screen", screen, |s, w| {
            parse_into(&mut s.agent.rows.auto.screen, w)
        }),
        word("agent.manual.inside", inside, |s, w| {
            parse_into(&mut s.agent.rows.manual.inside, w)
        }),
        word("agent.manual.other", other, |s, w| {
            parse_into(&mut s.agent.rows.manual.other, w)
        }),
        word("agent.manual.send", ask_or_runs, |s, w| {
            parse_into(&mut s.agent.rows.manual.send, w)
        }),
        word("agent.manual.delete", ask_or_runs, |s, w| {
            parse_into(&mut s.agent.rows.manual.delete, w)
        }),
        word("agent.manual.screen", screen, |s, w| {
            parse_into(&mut s.agent.rows.manual.screen, w)
        }),
    ]
}

/// Every key, in the order the schema lists them.
pub(crate) fn table() -> Vec<Key> {
    let mut keys = mode_keys();
    keys.extend(rest());
    keys
}

fn rest() -> Vec<Key> {
    let switch: &[&str] = &["off", "on"];
    vec![
        word("agent.mcp.expose", switch, |s, w| {
            parse_into(&mut s.expose, w)
        }),
        word("agent.acp.expose", switch, |s, w| parse_into(&mut s.acp, w)),
        word("agent.acp.agents", switch, |s, w| {
            parse_into(&mut s.agents, w)
        }),
        num("agent.budget.spend_microusd", 0..=1_000_000_000, |s, v| {
            // Millionths of a US dollar in the file. 0 is a cap of nothing: the turn after any
            // reported charge stops (a free or unpriced turn reports none).
            s.agent.budget.spend = Some(Money::usd(Decimal::new(v, 6)));
        }),
    ]
}
