//! `org.quire.Memory`, over memoryd: the planner reaches memory only through these router
//! actions, so the router can label what it delivers. Every request goes as the router's own
//! `Caller` for the Space of the invocation, and `memory.propose` always lands in the pending
//! queue (memoryd labels the router's proposals untrusted).

use almanac_client::{ClientError, Memory, Transport};
use almanac_core::{
    FactDraft, FactFilter, FactQuery, FactState, FactText, MemoryItem, RecallHit, RecallOver,
    RecallQuery, TopicPath,
};
use docket_client::IntentProvider;
use docket_core::{
    AppRefusal, EntityRef, FactLine, FailText, Follow, Hit, Invocation, LabelText, Outcome,
    ParamName, Preview, SuggestAsk, UndoFault, UndoToken, Undoable, ValidManifest, Value,
};
use porter_core::{AppName, Count};
use prov::{
    Actor, Confidentiality, EntityId, EntityKey, EntityKind, Integrity, Label, Labelled, Source,
    SpaceId,
};
use std::collections::BTreeSet;

/// The name the provider answers to.
pub const MEMORY_APP: &str = "org.quire.Memory";

/// How many hits or facts one call returns at most.
const LIMIT: Count = Count(10);

/// Where an agent's proposal is filed until the person keeps or drops it.
const PROPOSAL_TOPIC: &str = "notes/agent";

/// `org.quire.Memory`, over memoryd.
#[derive(Debug)]
pub struct MemoryProvider<T: Transport> {
    manifest: ValidManifest,
    memory: Memory<T>,
}

impl<T: Transport> MemoryProvider<T> {
    /// The provider for its shipped manifest, asking memoryd over `transport`.
    pub fn new(manifest: ValidManifest, transport: T) -> Self {
        Self {
            manifest,
            memory: Memory::over(transport),
        }
    }
}

fn app() -> AppName {
    AppName::parse(MEMORY_APP).expect("`org.quire.Memory` is a valid app name")
}

fn failed(why: &str) -> AppRefusal {
    AppRefusal::Failed(FailText(why.to_owned()))
}

/// What the router says about its own words: trusted metadata, private to the Space.
fn own_label(space: &SpaceId) -> Label {
    Label {
        integrity: Integrity::Trusted,
        confidentiality: Confidentiality::Private(BTreeSet::from([space.clone()])),
        classes: BTreeSet::new(),
        sources: BTreeSet::from([Source::App(app())]),
    }
}

fn unavailable(error: ClientError) -> AppRefusal {
    match error {
        ClientError::Transport(_) => failed("memory is not available"),
        ClientError::Refused(_) | ClientError::Unexpected => failed("memory said no"),
        _ => failed("memory is not available"),
    }
}

fn fact_entity(id: &almanac_core::FactId) -> Option<EntityId> {
    Some(EntityId {
        app: app(),
        kind: EntityKind::parse("memory.fact").ok()?,
        key: EntityKey::parse(id.as_str()).ok()?,
    })
}

fn text_param(inv: &Invocation, name: &str) -> Result<String, AppRefusal> {
    let param = ParamName::parse(name).map_err(|_| AppRefusal::Unsupported)?;
    match inv.args.get(&param).map(|a| &a.value) {
        Some(Value::Text(t)) if !t.trim().is_empty() => Ok(t.clone()),
        Some(Value::Text(_)) | None => Err(AppRefusal::NeedsParam {
            param,
            options: Vec::new(),
        }),
        Some(_) => Err(failed("that parameter is text")),
    }
}

fn line(label: &str, text: String, label_of: Label) -> Option<FactLine> {
    Some(FactLine {
        label: LabelText::parse(label).ok()?,
        value: Labelled {
            value: text,
            label: label_of,
        },
    })
}

/// The outcome of a read: the facts among the hits as entities, labelled with the join of
/// every hit's label (so what reads them is tainted as they are), and each hit's words as a
/// labelled line for the screen.
fn found(space: &SpaceId, hits: Vec<RecallHit>) -> Outcome {
    let label = hits
        .iter()
        .fold(own_label(space), |acc, hit| acc.join(&hit.label));
    let ids: Vec<EntityId> = hits
        .iter()
        .filter_map(|hit| match &hit.doc {
            MemoryItem::Fact(id) => fact_entity(id),
            MemoryItem::Event(_) => None,
        })
        .collect();
    let lines: Vec<FactLine> = hits
        .into_iter()
        .filter_map(|hit| {
            let what = match hit.doc {
                MemoryItem::Fact(_) => "Fact",
                MemoryItem::Event(_) => "Event",
            };
            line(what, hit.text.as_str().to_owned(), hit.label)
        })
        .collect();
    Outcome {
        value: Some(Labelled {
            value: Value::Entities(ids),
            label,
        }),
        said: LabelText::parse(&format!("Found {}", plural(lines.len()))).ok(),
        show: Preview::Facts(lines),
        undo: Undoable::No,
        follow: Follow::Nothing,
    }
}

fn plural(n: usize) -> String {
    match n {
        1 => "1 thing".to_owned(),
        n => format!("{n} things"),
    }
}

impl<T: Transport> MemoryProvider<T> {
    async fn recall(&self, inv: &Invocation) -> Result<Outcome, AppRefusal> {
        let text = text_param(inv, "query")?;
        let hits = self
            .memory
            .search(RecallQuery {
                space: inv.space.clone(),
                text: text.into(),
                limit: LIMIT,
                over: RecallOver::Both,
            })
            .await
            .map_err(unavailable)?;
        Ok(found(&inv.space, hits))
    }

    async fn facts(&self, inv: &Invocation) -> Result<Outcome, AppRefusal> {
        let facts = self
            .memory
            .facts(FactQuery {
                space: inv.space.clone(),
                topic: None,
                about: None,
                state: FactFilter::Active,
                limit: LIMIT,
            })
            .await
            .map_err(unavailable)?;
        let label = facts
            .iter()
            .fold(own_label(&inv.space), |acc, f| acc.join(&f.fact.label));
        let ids: Vec<EntityId> = facts
            .iter()
            .filter_map(|f| fact_entity(&f.fact.id))
            .collect();
        let lines: Vec<FactLine> = facts
            .iter()
            .filter_map(|f| {
                line(
                    "Fact",
                    f.fact.text.as_str().to_owned(),
                    f.fact.label.clone(),
                )
            })
            .collect();
        Ok(Outcome {
            value: Some(Labelled {
                value: Value::Entities(ids),
                label,
            }),
            said: LabelText::parse(&format!("Listed {}", plural(lines.len()))).ok(),
            show: Preview::Facts(lines),
            undo: Undoable::No,
            follow: Follow::Nothing,
        })
    }

    async fn propose(&self, inv: &Invocation) -> Result<Outcome, AppRefusal> {
        let text = text_param(inv, "text")?;
        let draft = FactDraft {
            topic: TopicPath::parse(PROPOSAL_TOPIC).map_err(|_| AppRefusal::Unsupported)?,
            text: FactText::parse(&text).map_err(|_| failed("that is not one short paragraph"))?,
            links: Vec::new(),
            supersedes: Vec::new(),
        };
        let (_, state) = self
            .memory
            .propose(inv.space.clone(), draft)
            .await
            .map_err(unavailable)?;
        // The router cannot settle a fact (only the shell's own UI can), so a proposal has no
        // undo here: the person drops it from the pending list.
        let said = match state {
            FactState::Pending => "Saved for you to keep or drop",
            FactState::Active | FactState::Superseded { .. } => "Remembered",
        };
        Ok(Outcome {
            value: None,
            said: LabelText::parse(said).ok(),
            show: Preview::None,
            undo: Undoable::No,
            follow: Follow::Nothing,
        })
    }
}

impl<T: Transport> IntentProvider for MemoryProvider<T> {
    fn manifest(&self) -> &ValidManifest {
        &self.manifest
    }

    async fn perform(&self, inv: Invocation) -> Result<Outcome, AppRefusal> {
        match inv.action.as_str() {
            "memory.recall" => self.recall(&inv).await,
            "memory.facts" => self.facts(&inv).await,
            "memory.propose" => self.propose(&inv).await,
            // Forgetting is the person's, from the memory view: the router may plan a forget
            // but only the shell's own UI may apply one, and the action is hidden from agents.
            _ => Err(AppRefusal::Unsupported),
        }
    }

    async fn dry_run(&self, inv: Invocation) -> Result<Preview, AppRefusal> {
        let _ = inv;
        Ok(Preview::None)
    }

    async fn undo(&self, token: UndoToken, actor: Actor) -> Result<(), UndoFault> {
        let _ = (token, actor);
        Err(UndoFault::Gone)
    }

    async fn search(&self, text: &str) -> Vec<Hit> {
        let _ = text;
        Vec::new()
    }

    async fn preview(&self, id: &EntityId) -> Preview {
        let _ = id;
        Preview::None
    }

    async fn suggest(&self, ask: SuggestAsk) -> Vec<EntityRef> {
        let _ = ask;
        Vec::new()
    }
}
