//! `FakeMail`: an in-memory mail app behind the fixture manifest, with real undo tokens.

use crate::labels::{app_label, entity, entity_ref, outcome, third_party};
use docket_client::IntentProvider;
use docket_core::{
    AppRefusal, Hit, Invocation, MessageSnip, Outcome, ParamName, Preview, SuggestAsk, TargetValue,
    UndoFault, UndoToken, Undoable, ValidManifest, Value,
};
use porter_core::{AppName, DataClass};
use prov::{Actor, EntityId, Labelled, Source, SpaceId, UnixSeconds};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

/// One thread in the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailThread {
    /// Its key.
    pub key: String,
    /// The subject, somebody else's words.
    pub subject: String,
    /// The sender's address.
    pub from: String,
    /// The body, somebody else's words.
    pub body: String,
}

/// One contact in the store (a trusted store: the app's own).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailContact {
    /// Its key.
    pub key: String,
    /// The display name.
    pub name: String,
    /// The address.
    pub address: String,
}

/// A message the app sent or forwarded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentMail {
    /// The contact key it went to.
    pub to: String,
    /// The thread keys forwarded, empty for a fresh message.
    pub threads: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum UndoStep {
    Unarchive(Vec<String>),
    Unsend(usize),
    Undraft(usize),
}

#[derive(Debug, Default)]
struct State {
    threads: BTreeMap<String, MailThread>,
    contacts: BTreeMap<String, MailContact>,
    archived: BTreeSet<String>,
    drafts: Vec<String>,
    sent: Vec<SentMail>,
    opened: Vec<String>,
    undo: BTreeMap<String, UndoStep>,
    next: u64,
}

/// The fake mail app.
#[derive(Debug)]
pub struct FakeMail {
    manifest: ValidManifest,
    app: AppName,
    space: SpaceId,
    state: Mutex<State>,
}

impl FakeMail {
    /// A mail app with this manifest, working in `space`.
    pub fn new(manifest: ValidManifest, space: SpaceId) -> Self {
        let app = manifest.manifest().app.clone();
        Self {
            manifest,
            app,
            space,
            state: Mutex::new(State::default()),
        }
    }

    /// Adds a thread.
    pub fn with_thread(self, thread: MailThread) -> Self {
        self.add_thread(thread);
        self
    }

    /// Adds a contact.
    pub fn with_contact(self, contact: MailContact) -> Self {
        self.add_contact(contact);
        self
    }

    /// Adds a thread to a running app.
    pub fn add_thread(&self, thread: MailThread) {
        self.edit(|s| {
            s.threads.insert(thread.key.clone(), thread);
        });
    }

    /// Adds a contact to a running app.
    pub fn add_contact(&self, contact: MailContact) {
        self.edit(|s| {
            s.contacts.insert(contact.key.clone(), contact);
        });
    }

    /// Forgets every thread, contact, draft, send and undo token.
    pub fn clear(&self) {
        self.edit(|s| *s = State::default());
    }

    fn edit<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        match self.state.lock() {
            Ok(mut s) => f(&mut s),
            Err(p) => f(&mut p.into_inner()),
        }
    }

    /// Whether a thread is archived.
    pub fn is_archived(&self, key: &str) -> bool {
        self.edit(|s| s.archived.contains(key))
    }

    /// Whether a thread still exists.
    pub fn has_thread(&self, key: &str) -> bool {
        self.edit(|s| s.threads.contains_key(key))
    }

    /// The threads the person opened, in order.
    pub fn opened(&self) -> Vec<String> {
        self.edit(|s| s.opened.clone())
    }

    /// What was sent or forwarded.
    pub fn sent(&self) -> Vec<SentMail> {
        self.edit(|s| s.sent.clone())
    }

    /// How many drafts exist.
    pub fn drafts(&self) -> usize {
        self.edit(|s| s.drafts.len())
    }

    fn keys(target: &TargetValue) -> Vec<String> {
        match target {
            TargetValue::Entities(ids) => ids.iter().map(|e| e.key.as_str().to_owned()).collect(),
            TargetValue::Nothing | TargetValue::Text(_) | TargetValue::Files(_) => vec![],
        }
    }

    fn token(s: &mut State) -> Option<UndoToken> {
        s.next += 1;
        UndoToken::parse(&format!("mail-undo-{}", s.next)).ok()
    }

    fn contact_key(inv: &Invocation) -> Option<String> {
        let name = ParamName::parse("to").ok()?;
        match &inv.args.get(&name)?.value {
            Value::Entity(e) => Some(e.key.as_str().to_owned()),
            _ => None,
        }
    }
}

impl IntentProvider for FakeMail {
    fn manifest(&self) -> &ValidManifest {
        &self.manifest
    }

    async fn perform(&self, inv: Invocation) -> Result<Outcome, AppRefusal> {
        let keys = Self::keys(&inv.target);
        let space = self.space.clone();
        let app = self.app.clone();
        self.edit(|s| match inv.action.as_str() {
            "mail.thread.read" => {
                let key = keys.first().ok_or(AppRefusal::Unsupported)?;
                let thread = s.threads.get(key).ok_or_else(|| {
                    entity(&app, "mail.thread", key)
                        .map_or(AppRefusal::Unsupported, AppRefusal::NotFound)
                })?;
                let label = third_party(Source::Mail, DataClass::Mail, space);
                let mut out = outcome(None, Undoable::No, Preview::None);
                out.value = Some(Labelled {
                    value: Value::Text(thread.body.clone()),
                    label,
                });
                Ok(out)
            }
            "mail.thread.open" => {
                let key = keys.first().ok_or(AppRefusal::Unsupported)?;
                if !s.threads.contains_key(key) {
                    return Err(entity(&app, "mail.thread", key)
                        .map_or(AppRefusal::Unsupported, AppRefusal::NotFound));
                }
                s.opened.push(key.clone());
                Ok(outcome(None, Undoable::No, Preview::None))
            }
            "mail.thread.archive" => {
                let missing = keys.iter().find(|k| !s.threads.contains_key(*k));
                if let Some(k) = missing {
                    return Err(entity(&app, "mail.thread", k)
                        .map_or(AppRefusal::Unsupported, AppRefusal::NotFound));
                }
                s.archived.extend(keys.iter().cloned());
                let token = Self::token(s).ok_or(AppRefusal::Busy)?;
                s.undo
                    .insert(token.as_str().to_owned(), UndoStep::Unarchive(keys.clone()));
                Ok(outcome(
                    Some(format!("Archived {} threads", keys.len())),
                    Undoable::Yes(token),
                    Preview::None,
                ))
            }
            "mail.draft.create" => {
                s.drafts.push("draft".into());
                let token = Self::token(s).ok_or(AppRefusal::Busy)?;
                s.undo.insert(
                    token.as_str().to_owned(),
                    UndoStep::Undraft(s.drafts.len() - 1),
                );
                Ok(outcome(
                    Some("Draft saved".into()),
                    Undoable::Yes(token),
                    Preview::None,
                ))
            }
            "mail.message.send" => {
                let to = Self::contact_key(&inv).ok_or(AppRefusal::Unsupported)?;
                s.sent.push(SentMail {
                    to,
                    threads: vec![],
                });
                let token = Self::token(s).ok_or(AppRefusal::Busy)?;
                s.undo.insert(
                    token.as_str().to_owned(),
                    UndoStep::Unsend(s.sent.len() - 1),
                );
                Ok(outcome(
                    Some("Sent".into()),
                    Undoable::Yes(token),
                    Preview::None,
                ))
            }
            "mail.message.forward" => {
                let to = Self::contact_key(&inv).ok_or(AppRefusal::Unsupported)?;
                s.sent.push(SentMail {
                    to,
                    threads: keys.clone(),
                });
                Ok(outcome(
                    Some(format!("Forwarded {} threads", keys.len())),
                    Undoable::No,
                    Preview::None,
                ))
            }
            "mail.thread.delete" => {
                keys.iter().for_each(|k| {
                    s.threads.remove(k);
                });
                Ok(outcome(
                    Some(format!("Deleted {} threads", keys.len())),
                    Undoable::No,
                    Preview::None,
                ))
            }
            _ => Err(AppRefusal::Unsupported),
        })
    }

    async fn dry_run(&self, inv: Invocation) -> Result<Preview, AppRefusal> {
        let app = self.app.clone();
        self.edit(|s| match inv.action.as_str() {
            "mail.message.send" | "mail.message.forward" => {
                let key = Self::contact_key(&inv).ok_or(AppRefusal::Unsupported)?;
                let contact = s.contacts.get(&key).ok_or_else(|| {
                    entity(&app, "mail.contact", &key)
                        .map_or(AppRefusal::Unsupported, AppRefusal::NotFound)
                })?;
                let own = |t: &str| Labelled {
                    value: t.to_owned(),
                    label: app_label(&app),
                };
                Ok(Preview::Message {
                    to: vec![own(&contact.address)],
                    subject: own("(subject)"),
                    body: own("(body)"),
                })
            }
            _ => Ok(Preview::None),
        })
    }

    async fn undo(&self, token: UndoToken, _actor: Actor) -> Result<(), UndoFault> {
        self.edit(|s| match s.undo.remove(token.as_str()) {
            None => Err(UndoFault::Gone),
            Some(UndoStep::Unarchive(keys)) => {
                keys.iter().for_each(|k| {
                    s.archived.remove(k);
                });
                Ok(())
            }
            Some(UndoStep::Unsend(i)) => {
                if i < s.sent.len() {
                    s.sent.remove(i);
                }
                Ok(())
            }
            Some(UndoStep::Undraft(i)) => {
                if i < s.drafts.len() {
                    s.drafts.remove(i);
                }
                Ok(())
            }
        })
    }

    async fn search(&self, text: &str) -> Vec<Hit> {
        let app = self.app.clone();
        let space = self.space.clone();
        self.edit(|s| {
            s.threads
                .values()
                .filter(|t| t.subject.contains(text))
                .filter_map(|t| {
                    let id = entity(&app, "mail.thread", &t.key)?;
                    let label = third_party(Source::Mail, DataClass::Mail, space.clone());
                    let title = Labelled {
                        value: t.subject.clone(),
                        label: label.clone(),
                    };
                    let sub = Labelled {
                        value: t.from.clone(),
                        label,
                    };
                    Some(Hit {
                        entity: entity_ref(id, title, sub),
                        why: None,
                    })
                })
                .collect()
        })
    }

    async fn preview(&self, id: &EntityId) -> Preview {
        let space = self.space.clone();
        self.edit(|s| match s.threads.get(id.key.as_str()) {
            None => Preview::None,
            Some(t) => {
                let label = third_party(Source::Mail, DataClass::Mail, space);
                let say = |v: &str| Labelled {
                    value: v.to_owned(),
                    label: label.clone(),
                };
                Preview::Thread {
                    subject: say(&t.subject),
                    messages: vec![MessageSnip {
                        from: say(&t.from),
                        snippet: say(&t.body),
                        at: UnixSeconds(0),
                    }],
                }
            }
        })
    }

    async fn suggest(&self, ask: SuggestAsk) -> Vec<docket_core::EntityRef> {
        let app = self.app.clone();
        let own = app_label(&app);
        self.edit(|s| {
            s.contacts
                .values()
                .filter(|c| c.name.contains(&ask.typed) || c.address.contains(&ask.typed))
                .filter_map(|c| {
                    let id = entity(&app, "mail.contact", &c.key)?;
                    let title = Labelled {
                        value: c.name.clone(),
                        label: own.clone(),
                    };
                    let sub = Labelled {
                        value: c.address.clone(),
                        label: own.clone(),
                    };
                    Some(entity_ref(id, title, sub))
                })
                .collect()
        })
    }
}
