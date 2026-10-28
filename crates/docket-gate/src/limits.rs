//! "Always ask" and "never", the pure half: the arguments of `companion.limit.add` read as a level
//! and a reach, and the limit a generated rule puts into a ruling.

use docket_core::Invocation;
use docket_permissions_core::{
    AskReason, LimitLevel, LimitNote, Limited, Ruling, StandingGrantId, UndoOffer,
};
use docket_vocab::{ParamName, Value};
use prov::Effect;

fn param(name: &str) -> ParamName {
    ParamName::parse(name).expect("a fixed parameter name is valid")
}

/// The text argument `name` of `inv`, if it holds words.
pub fn text_arg(inv: &Invocation, name: &str) -> Option<String> {
    match inv.args.get(&param(name)).map(|a| &a.value) {
        Some(Value::Text(t)) if !t.trim().is_empty() => Some(t.clone()),
        _ => None,
    }
}

/// The choice argument `name` of `inv`, if it holds a choice.
pub fn choice_arg(inv: &Invocation, name: &str) -> Option<String> {
    match inv.args.get(&param(name)).map(|a| &a.value) {
        Some(Value::Choice(c)) => Some(c.to_string()),
        _ => None,
    }
}

/// The level the `level` choice names: `ask` or `never`. Nothing else is a level, so the
/// action cannot carry an allow.
pub fn level_of(choice: Option<&str>) -> Option<LimitLevel> {
    match choice {
        Some("ask") => Some(LimitLevel::AlwaysAsk),
        Some("never") => Some(LimitLevel::Never),
        _ => None,
    }
}

/// The weakest effect the `reach` choice covers.
pub fn reach_of(choice: Option<&str>) -> Option<Effect> {
    match choice {
        None | Some("changes") => Some(Effect::UndoableWrite),
        Some("anything") => Some(Effect::Read),
        Some("removals") => Some(Effect::Destructive),
        Some(_) => None,
    }
}

/// The "ask first" a generated rule put into `ruling`, as the sheet shows it: the rule's own
/// sentence and the id Undo removes. Like a stored limit, it outranks every standing grant.
pub fn rule_limit(ruling: &Ruling) -> Limited {
    let Ruling::Ask(why) = ruling else {
        return Limited::Free;
    };
    why.iter()
        .find_map(|reason| match reason {
            AskReason::Said { rule, why } => {
                StandingGrantId::parse(&rule.0).ok().map(|id| LimitNote {
                    id,
                    level: LimitLevel::AlwaysAsk,
                    why: why.clone(),
                    undo: UndoOffer::Offered,
                })
            }
            _ => None,
        })
        .map_or(Limited::Free, Limited::Ask)
}
