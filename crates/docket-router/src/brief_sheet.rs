//! The pure half of confirming a brief: spotting the confirm action, reading its sheet and
//! turning the sheet's answer into a confirmation or a refusal.

use crate::companion_names::BRIEF_CONFIRM;
use crate::prepared::Prepared;
use docket_core::{
    ActionRef, BriefSheet, BriefText, COMPANION_APP, ConfirmAnswer, ConfirmEnd, GrantScope,
    LabelText, ParamName, Value,
};

/// The most pieces a sheet lists.
const PIECES_SHOWN: usize = 12;

/// Whether `action` is the one that asks the person to check a brief.
pub(crate) fn is_brief_confirm(action: &ActionRef) -> bool {
    action.app.as_str() == COMPANION_APP && action.name.as_str() == BRIEF_CONFIRM
}

pub(crate) fn text_of(p: &Prepared, name: &str) -> Option<String> {
    let name = ParamName::parse(name).ok()?;
    match p.request.args.get(&name).map(|held| &held.value) {
        Some(Value::Text(text)) => Some(text.clone()),
        _ => None,
    }
}

/// The sheet's brief for a call to `companion.brief.confirm`: the planner's draft, the stretches
/// to look at (found here, never by the planner) and the pieces it named. None for any other
/// call, and for a draft that is empty or longer than a brief may be: that sheet shows the lines
/// alone and cannot confirm anything.
pub(crate) fn sheet_of(p: &Prepared) -> Option<BriefSheet> {
    if !is_brief_confirm(&p.request.action) {
        return None;
    }
    let text = BriefText::parse(&text_of(p, "brief")?).ok()?;
    let title = text_of(p, "title")
        .and_then(|t| LabelText::parse(t.trim()).ok())
        .or_else(|| LabelText::parse("The brief").ok())?;
    let pieces = text_of(p, "parts")
        .unwrap_or_default()
        .lines()
        .filter_map(|line| LabelText::parse(line.trim()).ok())
        .take(PIECES_SHOWN)
        .collect();
    Some(BriefSheet::of(title, text, pieces))
}

/// What a brief sheet's answer comes to. A yes counts only when it carries the person's words,
/// and only once: any other answer (a yes from a surface that cannot edit, an "always", a
/// "never", a terminal grant) is not a confirmation, and nothing is kept. Other calls' answers
/// pass unchanged.
pub(crate) fn brief_answer(p: &Prepared, answer: ConfirmAnswer) -> ConfirmAnswer {
    if !is_brief_confirm(&p.request.action) {
        return answer;
    }
    match answer {
        ConfirmAnswer::Allowed {
            receipt,
            written: Some(words),
            ..
        } => ConfirmAnswer::Allowed {
            scope: GrantScope::Once,
            receipt,
            written: Some(words),
        },
        ConfirmAnswer::Allowed { written: None, .. }
        | ConfirmAnswer::AllowedFromTerminal { .. }
        | ConfirmAnswer::Never { .. } => ConfirmAnswer::Ended(ConfirmEnd::Refused),
        ended @ ConfirmAnswer::Ended(_) => ended,
    }
}
