//! The schema, the key table and the reader hold to one another.

use super::*;
use crate::keys::{Rule, table};
use std::collections::BTreeSet;

fn schema() -> toml::Table {
    SCHEMA.parse().expect("the schema is TOML")
}

fn rows() -> Vec<toml::Table> {
    schema()["key"]
        .as_array()
        .expect("key tables")
        .iter()
        .map(|k| k.as_table().expect("a table").clone())
        .collect()
}

fn path(row: &toml::Table) -> &str {
    row["path"].as_str().expect("path")
}

fn kind_of(row: &toml::Table) -> (&str, &toml::Table) {
    let kind = row["kind"].as_table().expect("kind");
    (
        kind["kind"].as_str().expect("kind name"),
        kind["v"].as_table().expect("kind v"),
    )
}

/// `text` for one key set to `value`, as a nested table document.
fn doc(path: &str, value: toml::Value) -> String {
    let mut table = toml::Table::new();
    let mut parts: Vec<&str> = path.split('.').collect();
    let leaf = parts.pop().expect("a leaf");
    let mut cursor = &mut table;
    for part in parts {
        cursor = cursor
            .entry(part)
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .expect("a table");
    }
    cursor.insert(leaf.to_owned(), value);
    table.to_string()
}

#[test]
fn the_schema_has_the_shape_the_settings_app_loads() {
    let schema = schema();
    assert_eq!(schema["app"].as_str(), Some("docket"));
    assert_eq!(schema["file"].as_str(), Some(SETTINGS_FILE));
    assert_eq!(schema["version"].as_integer(), Some(1));
    let mut seen = BTreeSet::new();
    for row in rows() {
        let p = path(&row);
        assert!(seen.insert(p.to_owned()), "duplicate {p}");
        assert!(p.starts_with("agent."), "{p}");
        for field in ["label", "help", "section"] {
            assert!(!row[field].as_str().expect(field).is_empty(), "{p} {field}");
        }
        assert!(
            matches!(row["exposure"].as_str(), Some("basic" | "advanced")),
            "{p}"
        );
        assert_eq!(row["page"]["kind"].as_str(), Some("intelligence"), "{p}");
        assert!(row.get("agent").is_none(), "{p} is never agent-settable");
        match kind_of(&row) {
            ("bounded", v) => {
                let (min, max) = (
                    v["min"].as_integer().unwrap(),
                    v["max"].as_integer().unwrap(),
                );
                let default = row["default"].as_integer().expect("a number default");
                assert!((min..=max).contains(&default), "{p}");
                assert!(!v["unit"].as_str().unwrap_or("").is_empty(), "{p} unit");
            }
            (kind @ ("toggle" | "segmented"), v) => {
                let words: Vec<&str> = v["variants"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(toml::Value::as_str)
                    .collect();
                assert_eq!(words.len() == 2, kind == "toggle", "{p}");
                assert!(
                    words.contains(&row["default"].as_str().expect("a word default")),
                    "{p}"
                );
                let labels = row["labels"].as_table().expect("labels for words");
                assert!(words.iter().all(|w| labels.contains_key(*w)), "{p}");
            }
            other => panic!("{p}: kind {other:?}"),
        }
    }
}

#[test]
fn the_intelligence_page_shows_privacy_and_the_rest_is_advanced() {
    let shown: Vec<String> = rows()
        .iter()
        .filter(|r| r["exposure"].as_str() == Some("basic"))
        .map(|r| path(r).to_owned())
        .collect();
    assert_eq!(
        shown,
        ["agent.strictness", "agent.mcp.expose", "agent.undo.keep_h"]
    );
}

#[test]
fn the_schema_rows_and_the_key_table_are_the_same_keys_with_the_same_ranges() {
    let keys = table();
    let rows = rows();
    let schema_paths: Vec<&str> = rows.iter().map(path).collect();
    let code_paths: Vec<&str> = keys.iter().map(|k| k.path).collect();
    assert_eq!(schema_paths, code_paths);
    for (row, key) in rows.iter().zip(&keys) {
        let (_, v) = kind_of(row);
        match &key.rule {
            Rule::Number { range, .. } => {
                assert_eq!(v["min"].as_integer(), Some(*range.start()), "{}", key.path);
                assert_eq!(v["max"].as_integer(), Some(*range.end()), "{}", key.path);
            }
            Rule::Word { words, .. } => {
                let schema_words: Vec<&str> = v["variants"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(toml::Value::as_str)
                    .collect();
                assert_eq!(&schema_words, words, "{}", key.path);
            }
        }
    }
}

#[test]
fn a_file_of_every_schema_default_changes_nothing() {
    let mut text = toml::Table::new();
    for row in rows() {
        let one: toml::Table = doc(path(&row), row["default"].clone()).parse().unwrap();
        for (k, v) in one {
            merge(&mut text, k, v);
        }
    }
    let loaded = read(&text.to_string(), AgentSettings::default());
    assert_eq!(loaded.value, AgentSettings::default());
    assert_eq!((loaded.fallbacks, loaded.unknown), (vec![], vec![]));
}

fn merged(parts: &[(&str, toml::Value)]) -> String {
    let mut text = toml::Table::new();
    for (p, v) in parts {
        for (k, v) in doc(p, v.clone()).parse::<toml::Table>().unwrap() {
            merge(&mut text, k, v);
        }
    }
    text.to_string()
}

fn merge(into: &mut toml::Table, key: String, value: toml::Value) {
    match (into.get_mut(&key), value) {
        (Some(toml::Value::Table(have)), toml::Value::Table(more)) => {
            for (k, v) in more {
                merge(have, k, v);
            }
        }
        (_, value) => {
            into.insert(key, value);
        }
    }
}

/// A valid value for the row that is not its default.
fn other_than_default(row: &toml::Table) -> toml::Value {
    match kind_of(row) {
        (_, v) if v.contains_key("min") => {
            let default = row["default"].as_integer().unwrap();
            let (min, max) = (
                v["min"].as_integer().unwrap(),
                v["max"].as_integer().unwrap(),
            );
            toml::Value::Integer(if default == max { min } else { max })
        }
        (_, v) => {
            let default = row["default"].as_str().unwrap();
            let word = v["variants"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(toml::Value::as_str)
                .find(|w| *w != default)
                .unwrap();
            toml::Value::String(word.to_owned())
        }
    }
}

#[test]
fn every_schema_key_is_read_by_the_daemon() {
    for row in rows() {
        let p = path(&row);
        let loaded = read(&doc(p, other_than_default(&row)), AgentSettings::default());
        assert_eq!(loaded.fallbacks, vec![], "{p}");
        assert_ne!(
            loaded.value,
            AgentSettings::default(),
            "{p} changed nothing"
        );
    }
}

#[test]
fn each_key_lands_in_the_typed_value_the_design_names() {
    use docket_core::{Depth, Millis, Seconds, Strictness};
    use porter_core::{Count, MicroUsd};
    let read_one = |p: &str, v: toml::Value| read(&doc(p, v), AgentSettings::default()).value;
    let int = toml::Value::Integer;
    let word = |w: &str| toml::Value::String(w.to_owned());
    assert_eq!(
        read_one("agent.strictness", word("trust_more"))
            .agent
            .strictness,
        Strictness::TrustMore
    );
    assert_eq!(
        read_one("agent.mcp.expose", word("on")).expose,
        McpExpose::On
    );
    assert_eq!(read_one("agent.acp.expose", word("on")).acp, AcpExpose::On);
    assert_eq!(
        read_one("agent.review.quick_ms", int(900))
            .agent
            .review
            .quick,
        Millis(900)
    );
    assert_eq!(
        read_one("agent.budget.calls", int(7)).agent.budget.calls,
        Count(7)
    );
    assert_eq!(
        read_one("agent.budget.chain", int(2)).agent.budget.chain,
        Depth(2)
    );
    assert_eq!(
        read_one("agent.budget.wall_s", int(120)).agent.budget.wall,
        Seconds(120)
    );
    assert_eq!(
        read_one("agent.budget.denials_in_a_row", int(5))
            .agent
            .breaker
            .consecutive,
        Count(5)
    );
    assert_eq!(
        read_one("agent.budget.spend_microusd", int(2_500_000))
            .agent
            .budget
            .spend,
        MicroUsd(2_500_000)
    );
    assert_eq!(
        read_one("agent.task_policy.max_min", int(30))
            .agent
            .task_policy_max,
        Seconds(1800)
    );
    assert_eq!(
        read_one("agent.undo.keep_h", int(48)).agent.undo_keep,
        Seconds(48 * 3600)
    );
}

#[test]
fn a_bad_value_falls_back_for_that_key_alone() {
    for row in rows() {
        let p = path(&row);
        let bad: Vec<toml::Value> = match kind_of(&row) {
            (_, v) if v.contains_key("min") => vec![
                toml::Value::Integer(v["min"].as_integer().unwrap() - 1),
                toml::Value::Integer(v["max"].as_integer().unwrap() + 1),
                toml::Value::String("many".into()),
                toml::Value::Boolean(true),
            ],
            _ => vec![
                toml::Value::String("reckless".into()),
                toml::Value::Integer(1),
                toml::Value::Boolean(true),
            ],
        };
        for value in bad {
            // A second, good key in the same file still applies.
            let other = if p == "agent.undo.keep_h" {
                "agent.budget.calls"
            } else {
                "agent.undo.keep_h"
            };
            let text = merged(&[(p, value.clone()), (other, toml::Value::Integer(100))]);
            let loaded = read(&text, AgentSettings::default());
            let mut want = AgentSettings::default();
            if other == "agent.undo.keep_h" {
                want.agent.undo_keep = docket_core::Seconds(100 * 3600);
            } else {
                want.agent.budget.calls = porter_core::Count(100);
            }
            assert_eq!(loaded.value, want, "{p} = {value}");
            assert_eq!(loaded.fallbacks.len(), 1, "{p} = {value}");
            assert_eq!(loaded.fallbacks[0].key, p);
        }
    }
}

#[test]
fn a_fallback_is_the_base_value_not_the_default() {
    let mut base = AgentSettings::default();
    base.agent.budget.calls = porter_core::Count(11);
    let loaded = read("[agent.budget]\ncalls = 0\n", base);
    assert_eq!(loaded.value, base);
    assert_eq!(
        loaded.fallbacks,
        vec![Fallback {
            key: "agent.budget.calls".into(),
            why: Why::OutOfRange {
                min: 1,
                max: 10_000
            },
        }]
    );
}

#[test]
fn a_file_that_is_not_toml_keeps_every_base_value_and_says_so() {
    let loaded = read("[agent.mcp\nexpose = \"on\"", AgentSettings::default());
    assert_eq!(loaded.value, AgentSettings::default());
    assert_eq!(loaded.fallbacks.len(), 1);
    assert!(matches!(loaded.fallbacks[0].why, Why::NotToml(_)));
}

#[test]
fn unknown_keys_are_reported_and_version_is_not_one() {
    let loaded = read(
        "version = 1\n[agent.budget]\nbogus = 1\n[agent.mcp]\nexpose = \"on\"\n[other]\nx = 1\n",
        AgentSettings::default(),
    );
    assert_eq!(loaded.value.expose, McpExpose::On);
    assert_eq!(loaded.unknown, ["agent.budget.bogus", "other.x"]);
    assert_eq!(
        loaded.lines("intentd"),
        [
            "intentd: settings: agent.budget.bogus: no such key, ignored",
            "intentd: settings: other.x: no such key, ignored"
        ]
    );
}

#[test]
fn the_locator_reads_the_first_file_of_the_configuration_directories() {
    let home = tempfile::tempdir().unwrap();
    let etc = tempfile::tempdir().unwrap();
    for (dir, text) in [
        (&home, "[agent.undo]\nkeep_h = 5\n"),
        (
            &etc,
            "[agent.undo]\nkeep_h = 9\n[agent.budget]\ncalls = 3\n",
        ),
    ] {
        std::fs::create_dir_all(dir.path().join("docket")).unwrap();
        std::fs::write(dir.path().join(SETTINGS_FILE), text).unwrap();
    }
    let env = |k: &str| match k {
        "XDG_CONFIG_HOME" => Some(home.path().display().to_string()),
        "XDG_CONFIG_DIRS" => Some(etc.path().display().to_string()),
        _ => None,
    };
    let locator = Locator::from_env(&env);
    assert_eq!(locator.watch_dir(), Some(home.path().join("docket")));
    let value = locator.read(AgentSettings::default()).value;
    assert_eq!(value.agent.undo_keep, docket_core::Seconds(5 * 3600));
    // The first file wins whole: the later directory's other keys are not merged in.
    assert_eq!(value.agent.budget.calls, porter_core::Count(200));
    let none = Locator::from_env(&|_: &str| None::<String>);
    assert_eq!(
        none.read(AgentSettings::default()),
        Loaded::of(AgentSettings::default())
    );
}
