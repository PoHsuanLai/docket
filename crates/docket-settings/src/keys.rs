//! The key table: every `agent.*` key of design/22 section 3.27, its rule and where its value
//! goes. One row per schema row (`dist/settings/docket.settings.toml`); the tests hold the two
//! tables to each other, so a row without a reader or a reader without a row fails.

use crate::AgentSettings;
use crate::expose::{AcpAgents, AcpExpose, McpExpose};
use docket_permissions_core::{DeleteRow, InsideRow, Mode, OtherRow, ScreenRow, SendRow};
use porter_core::MicroUsd;
use std::ops::RangeInclusive;

/// What a key's value may be and how it lands in the settings.
#[derive(Clone)]
pub(crate) enum Rule {
    /// A whole number inside `range`, in the key's unit.
    Number {
        range: RangeInclusive<i64>,
        set: fn(&mut AgentSettings, i64),
    },
    /// One of `words`, by position.
    Word {
        words: &'static [&'static str],
        set: fn(&mut AgentSettings, usize),
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

const MODES: [Mode; 2] = [Mode::Auto, Mode::Manual];
/// What each old `agent.strictness` word (`ask_more`, `default`, `trust_more`) means: `ask_more` is
/// Manual, `default` and `trust_more` are Auto.
const LEGACY_STRICTNESS: [Mode; 3] = [Mode::Manual, Mode::Auto, Mode::Auto];
const INSIDE: [InsideRow; 2] = [InsideRow::Runs, InsideRow::Asks];
const OTHER: [OtherRow; 2] = [OtherRow::Check, OtherRow::Asks];
const SEND: [SendRow; 2] = [SendRow::Asks, SendRow::Runs];
const DELETE: [DeleteRow; 2] = [DeleteRow::Asks, DeleteRow::Runs];
const SCREEN: [ScreenRow; 2] = [ScreenRow::Once, ScreenRow::Each];
const EXPOSE: [McpExpose; 2] = [McpExpose::Off, McpExpose::On];
const ACP: [AcpExpose; 2] = [AcpExpose::Off, AcpExpose::On];
const AGENTS: [AcpAgents; 2] = [AcpAgents::Off, AcpAgents::On];

/// Keys an older settings file may still hold, read before the table so a new key beside them
/// wins. They are not schema rows.
pub(crate) fn legacy() -> Vec<Key> {
    vec![
        Key {
            path: "agent.strictness",
            rule: Rule::Word {
                words: &["ask_more", "default", "trust_more"],
                set: |s, i| s.agent.start_mode = LEGACY_STRICTNESS[i],
            },
        },
        // The single row of the first permissions release: its word is the start mode.
        Key {
            path: "agent.permissions",
            rule: Rule::Word {
                words: &["auto", "manual"],
                set: |s, i| s.agent.start_mode = MODES[i],
            },
        },
    ]
}

const fn word(
    path: &'static str,
    words: &'static [&'static str],
    set: fn(&mut AgentSettings, usize),
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
        word("agent.start_mode", &["auto", "manual"], |s, i| {
            s.agent.start_mode = MODES[i]
        }),
        word("agent.auto.inside", inside, |s, i| {
            s.agent.rows.auto.inside = INSIDE[i]
        }),
        word("agent.auto.other", other, |s, i| {
            s.agent.rows.auto.other = OTHER[i]
        }),
        word("agent.auto.send", ask_or_runs, |s, i| {
            s.agent.rows.auto.send = SEND[i]
        }),
        word("agent.auto.delete", ask_or_runs, |s, i| {
            s.agent.rows.auto.delete = DELETE[i]
        }),
        word("agent.auto.screen", screen, |s, i| {
            s.agent.rows.auto.screen = SCREEN[i]
        }),
        word("agent.manual.inside", inside, |s, i| {
            s.agent.rows.manual.inside = INSIDE[i]
        }),
        word("agent.manual.other", other, |s, i| {
            s.agent.rows.manual.other = OTHER[i]
        }),
        word("agent.manual.send", ask_or_runs, |s, i| {
            s.agent.rows.manual.send = SEND[i]
        }),
        word("agent.manual.delete", ask_or_runs, |s, i| {
            s.agent.rows.manual.delete = DELETE[i]
        }),
        word("agent.manual.screen", screen, |s, i| {
            s.agent.rows.manual.screen = SCREEN[i]
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
    vec![
        Key {
            path: "agent.mcp.expose",
            rule: Rule::Word {
                words: &["off", "on"],
                set: |s, i| s.expose = EXPOSE[i],
            },
        },
        Key {
            path: "agent.acp.expose",
            rule: Rule::Word {
                words: &["off", "on"],
                set: |s, i| s.acp = ACP[i],
            },
        },
        Key {
            path: "agent.acp.agents",
            rule: Rule::Word {
                words: &["off", "on"],
                set: |s, i| s.agents = AGENTS[i],
            },
        },
        num("agent.budget.spend_microusd", 0..=1_000_000_000, |s, v| {
            s.agent.budget.spend = MicroUsd(u64::try_from(v).unwrap_or(u64::MAX));
        }),
    ]
}
