//! Confirming a brief. A planner that read something untrusted may draft a brief, but only the
//! person's words may become a helper's instruction. So the draft goes to the person as editable
//! text on a sheet, and what comes back is kept here, in the session, as words the person
//! wrote: the router alone makes them, from the sheet's answer, never from anything a planner or
//! an app says.

use crate::companion::BRIEF_CONFIRM;
use crate::driven::Run;
use crate::handles::HandleValue;
use crate::labels::user_label;
use crate::prepared::Prepared;
use crate::router::Router;
use crate::seams::Seams;
use docket_core::Clock;
use docket_core::EventSink;
use docket_core::{
    ActionRef, AppRefusal, AuditRecord, BriefEdit, BriefSheet, BriefText, ConfirmAnswer,
    ConfirmEnd, FailText, GrantScope, LabelText, Outcome, ParamName, Preview, Undoable, Value,
};
use prov::{Labelled, Source, UnixSeconds};

/// How many confirmed briefs a session keeps; the oldest is dropped past it.
pub const BRIEFS_KEPT: usize = 8;

/// The most pieces a sheet lists.
const PIECES_SHOWN: usize = 12;

/// Words the person confirmed on a sheet, kept in the session like a turn they typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmedBrief {
    /// The field's content when the person pressed the button.
    pub text: BriefText,
    /// When.
    pub at: UnixSeconds,
    /// The call that asked.
    pub call: docket_core::CallId,
}

/// Whether `action` is the one that asks the person to check a brief.
pub(crate) fn is_brief_confirm(action: &ActionRef) -> bool {
    action.app.as_str() == crate::companion::COMPANION_APP && action.name.as_str() == BRIEF_CONFIRM
}

fn text_of(p: &Prepared, name: &str) -> Option<String> {
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

fn plain(words: &str) -> Option<LabelText> {
    LabelText::parse(words).ok()
}

impl<S: Seams> Router<S> {
    /// The call to `companion.brief.confirm` after the person said yes: keeps the words they
    /// confirmed in the session and holds them as a handle that is theirs, and answers with those
    /// words. This is the only place that makes a handle from `Source::User`.
    pub(crate) fn confirmed_brief(&self, p: &Prepared, run: &Run) -> Result<Outcome, AppRefusal> {
        let failed = |why: &str| AppRefusal::Failed(FailText(why.to_owned()));
        let words = run.written.clone().ok_or_else(|| failed("not confirmed"))?;
        let session = p
            .who
            .session
            .as_ref()
            .ok_or_else(|| failed("there is no task to keep it in"))?;
        let edit = match text_of(p, "brief").as_deref() == Some(words.as_str()) {
            true => BriefEdit::Kept,
            false => BriefEdit::Changed,
        };
        let at = self.seams.clock().now();
        {
            let mut st = self.locked();
            let record = st
                .sessions
                .get_mut(session)
                .ok_or_else(|| failed("the session is gone"))?;
            record.briefs.push(ConfirmedBrief {
                text: words.clone(),
                at,
                call: p.id,
            });
            if record.briefs.len() > BRIEFS_KEPT {
                record.briefs.remove(0);
            }
            record.handles.mint(
                Labelled {
                    value: HandleValue::Text(words.as_str().to_owned()),
                    label: user_label(),
                },
                Source::User,
            );
        }
        self.seams.sink().append(AuditRecord::BriefConfirmed {
            at,
            call: p.id,
            session: session.clone(),
            edit,
            bytes: u32::try_from(words.as_str().len()).unwrap_or(u32::MAX),
        });
        let said = match edit {
            BriefEdit::Kept => "You checked the brief",
            BriefEdit::Changed => "You checked the brief (you changed it)",
        };
        Ok(Outcome {
            value: Some(Labelled {
                value: Value::Text(words.as_str().to_owned()),
                label: user_label(),
            }),
            said: plain(said),
            show: Preview::None,
            undo: Undoable::No,
            follow: docket_core::Follow::Nothing,
        })
    }
}
