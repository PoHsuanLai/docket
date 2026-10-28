//! Parameters the router fills itself (`ParamDecl::filled`): the person's newest words, or the
//! app's own records about the conversation. The planner is never shown such a parameter and
//! `check_call` refuses a call that sends one; here the router puts it in just before the app is
//! called, in `Prepared::filled`, so the sheet, the digest and the gate see what the planner
//! sent and nothing more. A value that cannot be had is left out, and the call still runs.

use crate::gate::Pending;
use crate::labels::{fold_labels, user_label};
use crate::prepared::Prepared;
use crate::router::Router;
use crate::seams::Seams;
use almanac_core::{
    BodyMode, Cursor, EntriesQuery, KindPattern, MemoryReply, MemoryRequest, RecentEntry,
};
use docket_core::Filled;
use docket_core::MemoryLink;
use docket_session::session_thing;
use docket_vocab::{Args, Value};
use porter_core::Count;
use prov::{EntityId, Labelled, SpaceId};

/// The most records of history a call is given (the newest ones when the app has more).
const HISTORY_LIMIT: u32 = 200;
/// The most pages read to reach the newest records.
const HISTORY_PAGES: usize = 10;

/// What the conversation a call works for says.
struct Conversation {
    /// The thing it is in memory; none when the call works for no conversation.
    thing: Option<EntityId>,
    /// The newest thing the person typed in it.
    words: Option<String>,
}

impl<S: Seams> Router<S> {
    /// `prepared` with its router-filled arguments in place. A call whose action fills none is
    /// returned as it is, and so is one the gate has already refused.
    pub(crate) async fn with_filled(&self, mut prepared: Prepared) -> Prepared {
        let refused = matches!(prepared.pending, Pending::Refuse(_));
        if refused || prepared.decl.params.iter().all(|p| p.filled.is_none()) {
            return prepared;
        }
        let conversation = self.conversation_of_call(&prepared);
        let mut filled = Args::new();
        for param in &prepared.decl.params {
            let Some(how) = &param.filled else {
                continue;
            };
            let arg = match how {
                Filled::PersonWords => conversation.words.clone().map(|words| Labelled {
                    value: Value::Text(words),
                    label: user_label(),
                }),
                Filled::OwnHistory { kinds } => {
                    self.own_history(&prepared.space, &conversation, kinds)
                        .await
                }
            };
            if let Some(arg) = arg {
                filled.insert(param.name.clone(), arg);
            }
        }
        prepared.filled = filled;
        prepared
    }

    /// The conversation the call's session works for (the end of its chain of parents), and what
    /// the person last typed in it.
    fn conversation_of_call(&self, prepared: &Prepared) -> Conversation {
        let st = self.locked();
        let conversation = prepared
            .who
            .session
            .as_ref()
            .and_then(|session| st.conversation_of(session));
        let words = conversation
            .as_ref()
            .and_then(|id| st.sessions.get(id))
            .and_then(|record| record.turns.last())
            .map(|turn| turn.text.clone());
        Conversation {
            thing: conversation.as_ref().and_then(session_thing),
            words,
        }
    }

    /// The app's records of `kinds` about the conversation, as a JSON array of their bodies,
    /// oldest first, labelled with the join of the records' labels (untrusted screen labels stay
    /// untrusted). Memory is asked as the router, in the Space of the call; a refusal, a failure
    /// or no records leave the argument out.
    async fn own_history(
        &self,
        space: &SpaceId,
        conversation: &Conversation,
        kinds: &str,
    ) -> Option<Labelled<Value>> {
        let about = conversation.thing.clone()?;
        let kind = KindPattern::parse(kinds).ok()?;
        let mut entries: Vec<RecentEntry> = Vec::new();
        let mut after: Option<Cursor> = None;
        for _ in 0..HISTORY_PAGES {
            let query = EntriesQuery {
                kinds: vec![kind.clone()],
                about: Some(about.clone()),
                after: after.take(),
                limit: Count(HISTORY_LIMIT),
                bodies: BodyMode::Json,
            };
            let Ok(MemoryReply::Entries(page)) = self
                .seams
                .memory()
                .ask(MemoryRequest::Entries(space.clone(), query))
                .await
            else {
                return None;
            };
            let full = page.entries.len() >= usize::try_from(HISTORY_LIMIT).unwrap_or(usize::MAX);
            entries.extend(page.entries);
            match page.next {
                Some(cursor) if full => after = Some(cursor),
                _ => break,
            }
        }
        let kept = usize::try_from(HISTORY_LIMIT).unwrap_or(usize::MAX);
        let surplus = entries.len().saturating_sub(kept);
        let bodies: Vec<&str> = entries
            .iter()
            .skip(surplus)
            .filter_map(|e| e.body.as_ref().map(|b| b.as_str()))
            .collect();
        let label = fold_labels(
            entries
                .iter()
                .skip(surplus)
                .filter(|e| e.body.is_some())
                .map(|e| e.label.clone()),
        )?;
        Some(Labelled {
            value: Value::Text(format!("[{}]", bodies.join(","))),
            label,
        })
    }
}
