//! Confirming a brief. A planner that read something untrusted may draft a brief, but only the
//! person's words may become a helper's instruction. So the draft goes to the person as editable
//! text on a sheet, and what comes back is kept here, in the session, as words the person
//! wrote: the router alone makes them, from the sheet's answer, never from anything a planner or
//! an app says.

use crate::brief_sheet::text_of;
use crate::driven::Run;
use crate::prepared::Prepared;
use crate::router::Router;
use crate::seams::Seams;
use docket_context::handles::HandleValue;
use docket_context::labels::user_label;
use docket_context::state::{BRIEFS_KEPT, ConfirmedBrief};
use docket_core::EventSink;
use docket_core::{AppRefusal, AuditRecord, BriefEdit, FailText, Outcome, Preview, Undoable};
use docket_vocab::{Clock, LabelText, Value};
use prov::{Labelled, Source};

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
