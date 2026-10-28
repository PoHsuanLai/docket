//! Limits as rules in a file. The assistant's "ask first" and "never", and the person's "never"
//! from a sheet, are kept as forbid rules in a rules file, so the person can read them, and the
//! policy rules on them like on any other rule. Each rule is named by its limit's id, so the one
//! revoke path removes it.
//!
//! This is the text only: the generated rule for a limit, a file with it added, a file without
//! it. Nothing here reads a file or the system.

use crate::grant::GrantCaller;
use crate::limit::{Limit, LimitCaller, LimitKey, LimitLevel, LimitMaker, LimitScope};
use crate::standing::{Revocation, StandingGrantId};
use prov::{Effect, SpaceScope};

/// The rules file a limit is written to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleFile {
    /// The person's `never.cedar`: what they said "never" to on a sheet.
    Never,
    /// The assistant's `assistant.cedar`: what it was asked to check with the person before.
    Assistant,
}

impl RuleFile {
    /// The file a limit made by `by` goes to. The administrator's limits are not written: they
    /// come from the floor.
    pub fn of(by: LimitMaker) -> Option<RuleFile> {
        match by {
            LimitMaker::Person => Some(RuleFile::Never),
            LimitMaker::Assistant => Some(RuleFile::Assistant),
            LimitMaker::Floor => None,
        }
    }

    /// The file's name.
    pub fn name(self) -> &'static str {
        match self {
            RuleFile::Never => "never.cedar",
            RuleFile::Assistant => "assistant.cedar",
        }
    }
}

const HEADER: &str = "// Rules Quire wrote for you, one for each \"never\" or \"ask first\".\n\
// Remove one from Settings, or delete it here. Rules you add by hand stay as they are.";

/// Every effect, weakest first.
const EFFECTS: [Effect; 5] = [
    Effect::Read,
    Effect::UndoableWrite,
    Effect::Outbound,
    Effect::Destructive,
    Effect::Execute,
];

/// `text` as a quoted string of the rule language.
fn quoted(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn caller_terms(caller: &LimitCaller) -> Vec<String> {
    match caller {
        LimitCaller::AnyAgent => vec!["principal.kind != \"user\"".to_owned()],
        LimitCaller::Only(who) => {
            let kind = format!("principal.kind == {}", quoted(who.kind().slug()));
            match who {
                GrantCaller::App(app) => vec![
                    kind,
                    format!("principal == Quire::Caller::{}", quoted(app.as_str())),
                ],
                _ => vec![kind],
            }
        }
    }
}

fn scope_terms(scope: &LimitScope) -> Vec<String> {
    let app = |app: &str| format!("resource in Quire::App::{}", quoted(app));
    match scope {
        LimitScope::Everything => Vec::new(),
        LimitScope::App(name) => vec![app(name.as_str())],
        LimitScope::Action(action) => vec![
            app(action.app.as_str()),
            format!("resource == Quire::Act::{}", quoted(action.name.as_str())),
        ],
    }
}

fn space_terms(space: &SpaceScope) -> Vec<String> {
    match space {
        SpaceScope::Any => Vec::new(),
        SpaceScope::Only(space) => vec![format!("context.space == {}", quoted(space.as_str()))],
    }
}

/// Calls with this effect or a stronger one; every call when `from` is a read.
fn effect_terms(from: Effect) -> Vec<String> {
    if from == Effect::Read {
        return Vec::new();
    }
    let slugs: Vec<String> = EFFECTS
        .into_iter()
        .filter(|e| *e >= from)
        .map(|e| quoted(e.slug()))
        .collect();
    vec![format!("[{}].contains(resource.effect)", slugs.join(", "))]
}

fn terms(key: &LimitKey) -> String {
    let all: Vec<String> = caller_terms(&key.caller)
        .into_iter()
        .chain(scope_terms(&key.scope))
        .chain(space_terms(&key.space))
        .chain(effect_terms(key.from))
        .collect();
    all.join(" && ")
}

/// The generated rule for `limit`: a forbid named by the limit's id, with the plain sentence
/// the sheet shows. "Never" refuses the call; "ask first" makes it ask, whatever is granted.
pub fn rule_text(limit: &Limit) -> String {
    let action = match limit.key.level {
        LimitLevel::Never => "action == Quire::Action::\"perform\"",
        LimitLevel::AlwaysAsk => {
            "action in [Quire::Action::\"perform_unasked\", Quire::Action::\"perform_unjudged\"]"
        }
    };
    format!(
        "@id({})\n@why({})\nforbid (principal, {action}, resource)\nwhen {{ {} }};",
        quoted(limit.id.as_str()),
        quoted(&limit.note().why),
        terms(&limit.key),
    )
}

/// Whether a block of a rules file is the rule `id`.
fn is_rule(block: &str, id: &StandingGrantId) -> bool {
    block
        .trim_start()
        .starts_with(&format!("@id({})", quoted(id.as_str())))
}

fn blocks(file: &str) -> impl Iterator<Item = &str> {
    file.split("\n\n").map(str::trim).filter(|b| !b.is_empty())
}

fn joined<'a>(blocks: impl Iterator<Item = &'a str>) -> String {
    let all: Vec<&str> = blocks.collect();
    format!("{}\n", all.join("\n\n"))
}

/// `file` without the rule `id`, and whether it held one. Everything else in the file stays.
pub fn without_rule(file: &str, id: &StandingGrantId) -> (String, Revocation) {
    let held = blocks(file).any(|b| is_rule(b, id));
    let done = if held {
        Revocation::Revoked
    } else {
        Revocation::NotHeld
    };
    (joined(blocks(file).filter(|b| !is_rule(b, id))), done)
}

/// `file` with the rule for `limit`; the same id replaces the earlier rule. An empty file
/// starts with a short note on what the file is.
pub fn with_rule(file: &str, limit: &Limit) -> String {
    let (rest, _) = without_rule(file, &limit.id);
    let head = if rest.trim().is_empty() {
        HEADER.to_owned()
    } else {
        rest.trim_end().to_owned()
    };
    format!("{head}\n\n{}\n", rule_text(limit))
}

/// The limit id a generated rule's first line names, if `line` is one.
fn generated_id(line: &str) -> Option<StandingGrantId> {
    let id = line
        .trim_start()
        .strip_prefix("@id(\"")?
        .split('"')
        .next()?;
    StandingGrantId::parse(id).ok()
}

/// The ids of the generated rules `file` holds.
pub fn rule_ids(file: &str) -> Vec<StandingGrantId> {
    blocks(file)
        .filter_map(|b| b.lines().next())
        .filter_map(generated_id)
        .collect()
}

/// How many lines a generated rule takes: its `@id`, its `@why`, its `forbid` and its `when`.
const RULE_LINES: usize = 4;

/// The generated rules `file` holds, each cut out of the file on its own, however the rest of the
/// file reads. A rules file the loader rejects (a hand edit that does not parse) must not take
/// the person's "never" with it: the loader keeps each of these that is good by itself.
pub fn generated_rules(file: &str) -> Vec<String> {
    let lines: Vec<&str> = file.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| generated_id(line).is_some())
        .map(|(at, _)| lines[at..lines.len().min(at + RULE_LINES)].join("\n"))
        .collect()
}
