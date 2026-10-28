//! The handle table. When the router hands anything to a companion session (context, search
//! hits, read outcomes, previews), every untrusted text is replaced by a handle; the text reaches
//! only the reader, and the screen through `Session.Display`. A model cannot forge taint: the
//! router computes it.

use docket_core::{FileRef, Handle, HandleCard, HandleShape, Reveal, size_of};
use docket_session::HandleLabel;
use prov::{EntityId, Integrity, Label, Labelled, Measured, Quarantined, Source};
use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};

/// The most handles one session holds. An app is the source of what fills a table (a search can
/// return thousands of things); a session that reaches this refuses the next result with
/// [`HandleLimit::Handles`] rather than dropping an older handle a planner may still name. A task
/// that needs more starts a new session.
pub const MAX_HANDLES: usize = 4096;

/// The most text handles the context view holds at once. Beyond this a new text of the view
/// evicts the oldest text handle the current view does not use; the evicted number is never
/// reused, and naming it refuses as an unknown handle. A view is bounded by what an app reports
/// (a capped list of things), so the current view always fits.
pub const MAX_VIEW_TEXTS: usize = 512;

/// The most things one session may have been shown (`SessionRecord::known`), for the same
/// reason and with the same refusal ([`HandleLimit::Known`]).
pub const MAX_KNOWN: usize = 4096;

/// Why a session cannot take in one more result. Both read, to a planner, as a call that
/// returned too many things (`BudgetKind::FanOut`), and the daemon logs which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HandleLimit {
    /// The session holds as many handles as it may.
    #[error("the session holds as many handles as it may")]
    Handles,
    /// The session has been shown as many things as it may.
    #[error("the session has been shown as many things as it may")]
    Known,
}

/// What a handle holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandleValue {
    /// Words.
    Text(String),
    /// A thing.
    Entity(EntityId),
    /// A file.
    File(FileRef),
    /// What a restart left of a handle: its label, shape and source, not its value. It cannot
    /// be shown, read or put in an argument; a fresh call mints a new handle for the thing.
    Forgotten(HandleShape),
}

impl Measured for HandleValue {
    fn measure(&self) -> usize {
        match self {
            HandleValue::Text(t) => t.len(),
            HandleValue::Entity(_) | HandleValue::File(_) => 1,
            HandleValue::Forgotten(_) => 0,
        }
    }
}

/// One held value with its label and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandleEntry {
    /// The value and its label.
    pub value: Labelled<HandleValue>,
    /// Where it came from.
    pub from: Source,
}

/// The values a session holds for readers. Router-owned; one per session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HandleTable {
    next: u64,
    entries: BTreeMap<Handle, HandleEntry>,
    /// The handles that hold each thing (one per distinct label), so a thing is found without
    /// a scan of `entries`.
    things: BTreeMap<EntityId, Vec<Handle>>,
    /// Handles minted since the session's record last took them (see `take_fresh`).
    fresh: Vec<Handle>,
    /// The text handles that hold each text (by a hash of it), so the same text under the same
    /// label and source keeps its number for the life of the task.
    texts: BTreeMap<u64, Vec<Handle>>,
    /// The text handles the context view minted, with the view (`epoch`) that last used them:
    /// the ones eligible for eviction.
    view_seen: BTreeMap<Handle, u64>,
    /// Which context view is being built; see [`begin_view`](Self::begin_view).
    epoch: u64,
}

fn text_key(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

impl HandleTable {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Holds a value and returns its handle, past [`MAX_HANDLES`] too: for the router's own
    /// bounded values. What an app returns goes through [`try_mint`](Self::try_mint).
    pub fn mint(&mut self, value: Labelled<HandleValue>, from: Source) -> Handle {
        self.next += 1;
        let handle = Handle(self.next);
        if let HandleValue::Entity(thing) = &value.value {
            self.things.entry(thing.clone()).or_default().push(handle);
        }
        self.entries.insert(handle, HandleEntry { value, from });
        self.fresh.push(handle);
        handle
    }

    /// Whether the table holds [`MAX_HANDLES`] already.
    pub fn is_full(&self) -> bool {
        self.entries.len() >= MAX_HANDLES
    }

    /// [`mint`](Self::mint), refused when the table is full. Nothing is evicted.
    pub fn try_mint(
        &mut self,
        value: Labelled<HandleValue>,
        from: Source,
    ) -> Result<Handle, HandleLimit> {
        if self.is_full() {
            return Err(HandleLimit::Handles);
        }
        Ok(self.mint(value, from))
    }

    /// Holds a handle a restart brought back as a label: the number is kept (a new handle never
    /// reuses it) and the value is not.
    pub fn restore_label(&mut self, label: HandleLabel) {
        self.next = self.next.max(label.handle.0);
        let value = Labelled {
            value: HandleValue::Forgotten(label.shape),
            label: label.label,
        };
        self.entries.insert(
            label.handle,
            HandleEntry {
                value,
                from: label.from,
            },
        );
    }

    /// The handles minted since the last call, as the log keeps them: label, shape and source,
    /// never the value. Handles restored as labels are not fresh.
    pub fn take_fresh(&mut self) -> Vec<HandleLabel> {
        std::mem::take(&mut self.fresh)
            .into_iter()
            .filter_map(|handle| {
                let card = self.card(handle)?;
                let entry = self.entries.get(&handle)?;
                Some(HandleLabel {
                    handle,
                    label: entry.value.label.clone(),
                    shape: card.shape,
                    from: card.from,
                })
            })
            .collect()
    }

    /// Whether `take_fresh` has anything.
    pub fn has_fresh(&self) -> bool {
        !self.fresh.is_empty()
    }

    /// Where the next handle will be numbered from: a mark for `untrusted_since`.
    pub fn mark(&self) -> u64 {
        self.next
    }

    /// Whether a handle numbered after `mark` holds untrusted text or things.
    pub fn untrusted_since(&self, mark: u64) -> bool {
        self.entries
            .iter()
            .any(|(h, e)| h.0 > mark && e.value.label.integrity == Integrity::Untrusted)
    }

    /// The handles a restart left as labels, in order.
    pub fn forgotten(&self) -> Vec<(Handle, &Label)> {
        self.entries
            .iter()
            .filter(|(_, e)| matches!(e.value.value, HandleValue::Forgotten(_)))
            .map(|(h, e)| (*h, &e.value.label))
            .collect()
    }

    /// The handle that already holds `thing` under `label`, if any.
    fn handle_of(&self, thing: &EntityId, label: &Label) -> Option<Handle> {
        self.things
            .get(thing)?
            .iter()
            .copied()
            .find(|h| self.entries.get(h).is_some_and(|e| &e.value.label == label))
    }

    /// Holds a thing and returns its handle: the one it already has when the same thing is held
    /// with the same label, so the same search twice names the same `#n` and a planner can see
    /// it got nothing new.
    pub fn mint_entity(&mut self, thing: EntityId, label: Label) -> Handle {
        match self.handle_of(&thing, &label) {
            Some(known) => known,
            None => {
                let from = Source::App(thing.app.clone());
                self.mint(
                    Labelled {
                        value: HandleValue::Entity(thing),
                        label,
                    },
                    from,
                )
            }
        }
    }

    /// [`mint_entity`](Self::mint_entity), refused when the thing is new and the table is full.
    pub fn try_mint_entity(
        &mut self,
        thing: EntityId,
        label: Label,
    ) -> Result<Handle, HandleLimit> {
        match self.handle_of(&thing, &label) {
            Some(known) => Ok(known),
            None if self.is_full() => Err(HandleLimit::Handles),
            None => Ok(self.mint_entity(thing, label)),
        }
    }

    /// Starts a context view: texts the view then uses are kept, and an older view's texts that
    /// it does not use may be evicted once the view holds [`MAX_VIEW_TEXTS`].
    pub fn begin_view(&mut self) {
        self.epoch += 1;
    }

    fn text_handle(&self, text: &Labelled<String>, from: &Source) -> Option<Handle> {
        self.texts
            .get(&text_key(&text.value))?
            .iter()
            .copied()
            .find(|h| {
                self.entries.get(h).is_some_and(|e| {
                    e.from == *from
                        && e.value.label == text.label
                        && matches!(&e.value.value, HandleValue::Text(t) if *t == text.value)
                })
            })
    }

    fn mint_text(&mut self, text: Labelled<String>, from: Source) -> Handle {
        let key = text_key(&text.value);
        let handle = self.mint(
            Labelled {
                value: HandleValue::Text(text.value),
                label: text.label,
            },
            from,
        );
        self.texts.entry(key).or_default().push(handle);
        handle
    }

    /// Holds a text and returns its handle: the one it already has when the same text is held
    /// with the same label and source, so a thing a planner was shown keeps its number across
    /// turns. Refused when the text is new and the table is full.
    pub fn try_mint_text(
        &mut self,
        text: Labelled<String>,
        from: Source,
    ) -> Result<Handle, HandleLimit> {
        match self.text_handle(&text, &from) {
            Some(known) => Ok(known),
            None if self.is_full() => Err(HandleLimit::Handles),
            None => Ok(self.mint_text(text, from)),
        }
    }

    fn forget_oldest_unused_view_text(&mut self) {
        if self.view_seen.len() < MAX_VIEW_TEXTS {
            return;
        }
        let epoch = self.epoch;
        let Some(oldest) = self
            .view_seen
            .iter()
            .find(|(_, seen)| **seen < epoch)
            .map(|(h, _)| *h)
        else {
            return;
        };
        self.view_seen.remove(&oldest);
        self.entries.remove(&oldest);
        self.texts
            .values_mut()
            .for_each(|hs| hs.retain(|h| *h != oldest));
        self.texts.retain(|_, hs| !hs.is_empty());
    }

    /// Words for a planner: plain when trusted, a handle when not. The handle is the one the
    /// same words already have (see [`try_mint_text`](Self::try_mint_text)).
    pub fn reveal(&mut self, text: Labelled<String>, from: Source) -> Reveal<String> {
        match text.label.integrity {
            Integrity::Trusted => Reveal::Plain(text.value),
            Integrity::Untrusted => {
                let handle = match self.text_handle(&text, &from) {
                    Some(known) => known,
                    None => {
                        self.forget_oldest_unused_view_text();
                        self.mint_text(text, from)
                    }
                };
                self.view_seen.insert(handle, self.epoch);
                Reveal::Handle(handle)
            }
        }
    }

    /// What a planner is told about a handle: its shape, source and size, never its content.
    /// A handle a restart left as a label is no one's to use: no card.
    pub fn card(&self, handle: Handle) -> Option<HandleCard> {
        let entry = self.entries.get(&handle)?;
        let (shape, size) = match &entry.value.value {
            HandleValue::Text(t) => (HandleShape::Text, size_of(t)),
            HandleValue::Entity(e) => (
                HandleShape::Entity(e.kind.clone()),
                docket_core::CharCount(0),
            ),
            HandleValue::File(_) => (HandleShape::File, docket_core::CharCount(0)),
            HandleValue::Forgotten(_) => return None,
        };
        Some(HandleCard {
            handle,
            shape,
            from: entry.from.clone(),
            size,
        })
    }

    /// Cards for every handle a planner may use, in order (a restart's labels have none).
    pub fn cards(&self) -> Vec<HandleCard> {
        self.entries.keys().filter_map(|h| self.card(*h)).collect()
    }

    /// What a handle holds, with its label, for the router to put back in an argument.
    pub fn value(&self, handle: Handle) -> Option<&Labelled<HandleValue>> {
        self.entries.get(&handle).map(|e| &e.value)
    }

    /// A handle's label.
    pub fn label(&self, handle: Handle) -> Option<&Label> {
        self.entries.get(&handle).map(|e| &e.value.label)
    }

    /// A text handle's content for the reader (`Session.Resolve`, the reader role only), still
    /// quarantined: only the reader's host holds the key that opens it.
    pub fn resolve_text(&self, handle: Handle) -> Option<Quarantined<String>> {
        let entry = self.entries.get(&handle)?;
        match &entry.value.value {
            HandleValue::Text(t) => Some(Quarantined::new(Labelled {
                value: t.clone(),
                label: entry.value.label.clone(),
            })),
            HandleValue::Entity(_) | HandleValue::File(_) | HandleValue::Forgotten(_) => None,
        }
    }

    /// A text handle's content for the screen (`Session.Display`, never for a model).
    pub fn display(&self, handle: Handle) -> Option<&str> {
        match &self.entries.get(&handle)?.value.value {
            HandleValue::Text(t) => Some(t),
            HandleValue::Entity(_) | HandleValue::File(_) | HandleValue::Forgotten(_) => None,
        }
    }
}
