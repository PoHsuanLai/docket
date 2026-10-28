//! The acceptance run's mail app: `IntentProvider1` over the scratch mailbox.

use super::{AppLog, Focus, Message, Sending, contacts, threads, words_match};
use docket_core::{
    AppRefusal, Follow, Hit, Invocation, MessageSnip, Outcome, Preview, SuggestAsk, UndoFault,
    Undoable, ValidManifest,
};
use docket_provider::IntentProvider;
use docket_vocab::{EntityRef, LabelText, ParamName, TargetValue, UndoToken, Value};
use porter_core::{AppName, DataClass};
use prov::{
    Actor, Confidentiality, EntityId, EntityKey, EntityKind, Integrity, Label, Labelled, Source,
    SpaceId, UnixSeconds,
};
use std::collections::BTreeSet;

/// The app.
#[derive(Debug)]
pub struct AcceptMail {
    manifest: ValidManifest,
    app: AppName,
    space: SpaceId,
    log: AppLog,
    focus: Focus,
}

impl AcceptMail {
    /// A mail app with this manifest working in `space`, and the log the test reads.
    pub fn new(manifest: ValidManifest, space: SpaceId) -> (Self, AppLog) {
        let log = AppLog::default();
        let app = manifest.manifest().app.clone();
        (
            Self {
                manifest,
                app,
                space,
                log: log.clone(),
                focus: Focus::Nowhere,
            },
            log,
        )
    }

    /// The same app with `focus` open in its window.
    pub fn focused(self, focus: Focus) -> Self {
        Self { focus, ..self }
    }

    fn id(&self, kind: &str, key: &str) -> Option<EntityId> {
        Some(EntityId {
            app: self.app.clone(),
            kind: EntityKind::parse(kind).ok()?,
            key: EntityKey::parse(key).ok()?,
        })
    }

    fn own(&self) -> Label {
        Label {
            integrity: Integrity::Trusted,
            confidentiality: Confidentiality::Public,
            classes: BTreeSet::new(),
            sources: BTreeSet::from([Source::App(self.app.clone())]),
        }
    }

    fn theirs(&self) -> Label {
        Label::untrusted(Source::Mail, DataClass::Mail, self.space.clone())
    }

    fn labelled(&self, text: &str, label: &Label) -> Labelled<String> {
        Labelled {
            value: text.to_owned(),
            label: label.clone(),
        }
    }

    fn entity_ref(&self, id: EntityId, title: &str, sub: &str, label: &Label) -> EntityRef {
        EntityRef {
            id,
            title: self.labelled(title, label),
            subtitle: self.labelled(sub, label),
        }
    }

    fn keys(target: &TargetValue) -> Vec<String> {
        match target {
            TargetValue::Entities(ids) => ids.iter().map(|e| e.key.as_str().to_owned()).collect(),
            TargetValue::Nothing
            | TargetValue::Handles(_)
            | TargetValue::Text(_)
            | TargetValue::Files(_) => vec![],
        }
    }

    fn text_arg(inv: &Invocation, name: &str) -> Option<String> {
        match &inv.args.get(&ParamName::parse(name).ok()?)?.value {
            Value::Text(t) => Some(t.clone()),
            _ => None,
        }
    }

    fn contact_arg(inv: &Invocation) -> Option<String> {
        match &inv.args.get(&ParamName::parse("to").ok()?)?.value {
            Value::Entity(e) => Some(e.key.as_str().to_owned()),
            Value::Text(t) => Some(t.clone()),
            _ => None,
        }
    }

    fn done(said: &str, value: Option<Labelled<Value>>, undo: Undoable) -> Outcome {
        Outcome {
            value,
            said: LabelText::parse(said).ok(),
            show: Preview::None,
            undo,
            follow: Follow::Nothing,
        }
    }

    fn hold(&self, inv: &Invocation) -> Result<Outcome, AppRefusal> {
        let to = Self::contact_arg(inv).ok_or(AppRefusal::Unsupported)?;
        let message = Message {
            action: inv.action.as_str().to_owned(),
            to,
            threads: Self::keys(&inv.target),
            body: Self::text_arg(inv, "body").unwrap_or_default(),
            actor: inv.actor.clone(),
            state: Sending::Held,
        };
        let said = format!("Held {} for the undo window", inv.action.as_str());
        let token = self.log.hold(message);
        let token = UndoToken::parse(&token).map_err(|_| AppRefusal::Busy)?;
        Ok(Self::done(&said, None, Undoable::Yes(token)))
    }
}

impl IntentProvider for AcceptMail {
    fn manifest(&self) -> &ValidManifest {
        &self.manifest
    }

    async fn perform(&self, inv: Invocation) -> Result<Outcome, AppRefusal> {
        self.log.performing(inv.action.as_str());
        match inv.action.as_str() {
            "mail.thread.search" => {
                let query = Self::text_arg(&inv, "query")
                    .unwrap_or_default()
                    .to_lowercase();
                let ids: Vec<EntityId> = threads()
                    .iter()
                    .filter(|t| words_match(&query, t.subject))
                    .filter_map(|t| self.id("mail.thread", t.key))
                    .collect();
                Ok(Self::done(
                    "Found threads",
                    Some(Labelled {
                        value: Value::Entities(ids),
                        label: self.theirs(),
                    }),
                    Undoable::No,
                ))
            }
            "mail.thread.current" => {
                let ids: Vec<EntityId> = match self.focus {
                    Focus::Nowhere => vec![],
                    Focus::Thread(key) => self.id("mail.thread", key).into_iter().collect(),
                };
                Ok(Self::done(
                    "The open thread",
                    Some(Labelled {
                        value: Value::Entities(ids),
                        label: self.own(),
                    }),
                    Undoable::No,
                ))
            }
            "mail.thread.related" => {
                let key = Self::keys(&inv.target)
                    .into_iter()
                    .next()
                    .ok_or(AppRefusal::Unsupported)?;
                let thread = threads()
                    .into_iter()
                    .find(|t| t.key == key)
                    .ok_or(AppRefusal::Unsupported)?;
                // The sender is a contact when the address is saved, else the address itself
                // stands for them: it is the message's word, so the answer is theirs.
                let sender = contacts()
                    .iter()
                    .find(|c| c.address == thread.from)
                    .map_or(thread.from, |c| c.key);
                Ok(Self::done(
                    "Found the sender",
                    Some(Labelled {
                        value: Value::Entities(
                            self.id("mail.contact", sender).into_iter().collect(),
                        ),
                        label: self.theirs(),
                    }),
                    Undoable::No,
                ))
            }
            "mail.thread.read" => {
                let key = Self::keys(&inv.target)
                    .into_iter()
                    .next()
                    .ok_or(AppRefusal::Unsupported)?;
                self.log.reading(&key);
                let thread = threads()
                    .into_iter()
                    .find(|t| t.key == key)
                    .ok_or(AppRefusal::Unsupported)?;
                Ok(Self::done(
                    "Read the thread",
                    Some(Labelled {
                        value: Value::Text(thread.body.to_owned()),
                        label: self.theirs(),
                    }),
                    Undoable::No,
                ))
            }
            "mail.contact.search" => {
                let query = Self::text_arg(&inv, "query")
                    .unwrap_or_default()
                    .to_lowercase();
                let ids: Vec<EntityId> = contacts()
                    .iter()
                    .filter(|c| words_match(&query, c.name))
                    .filter_map(|c| self.id("mail.contact", c.key))
                    .collect();
                Ok(Self::done(
                    "Found contacts",
                    Some(Labelled {
                        value: Value::Entities(ids),
                        label: self.own(),
                    }),
                    Undoable::No,
                ))
            }
            "mail.message.forward" | "mail.message.send" => self.hold(&inv),
            _ => Err(AppRefusal::Unsupported),
        }
    }

    async fn dry_run(&self, inv: Invocation) -> Result<Preview, AppRefusal> {
        match inv.action.as_str() {
            "mail.message.forward" | "mail.message.send" => {
                let to = Self::contact_arg(&inv).ok_or(AppRefusal::Unsupported)?;
                let address = contacts()
                    .iter()
                    .find(|c| c.key == to)
                    .map_or(to.clone(), |c| c.address.to_owned());
                // A typed address is the person's or the planner's words, not the app's.
                let label = match contacts().iter().any(|c| c.key == to) {
                    true => self.own(),
                    false => self.theirs(),
                };
                Ok(Preview::Message {
                    to: vec![self.labelled(&address, &label)],
                    subject: self.labelled("(subject)", &self.own()),
                    body: self.labelled("(body)", &self.own()),
                })
            }
            _ => Ok(Preview::None),
        }
    }

    async fn undo(&self, token: UndoToken, _actor: Actor) -> Result<(), UndoFault> {
        self.log.cancel(token.as_str())
    }

    async fn search(&self, text: &str) -> Vec<Hit> {
        let own = self.theirs();
        threads()
            .iter()
            .filter(|t| words_match(text, t.subject))
            .filter_map(|t| {
                let id = self.id("mail.thread", t.key)?;
                Some(Hit {
                    entity: self.entity_ref(id, t.subject, t.from, &own),
                    why: None,
                })
            })
            .collect()
    }

    async fn preview(&self, id: &EntityId) -> Preview {
        let label = self.theirs();
        threads()
            .iter()
            .find(|t| t.key == id.key.as_str())
            .map_or(Preview::None, |t| Preview::Thread {
                subject: self.labelled(t.subject, &label),
                messages: vec![MessageSnip {
                    from: self.labelled(t.from, &label),
                    snippet: self.labelled(t.body, &label),
                    at: UnixSeconds(0),
                }],
            })
    }

    async fn suggest(&self, _ask: SuggestAsk) -> Vec<EntityRef> {
        vec![]
    }
}
