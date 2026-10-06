//! How an answer was reached: which model ran each stage, by which door, and why that one.
//!
//! inferd announces these as events (`Why`, `Stage`, `Declined`); companiond keeps the last
//! turn's as [`RouteNote`]s and serves them on `Companion1.Answer.Routing`. [`footer_line`] turns
//! the notes into the one line a card shows, so every surface prints the same words.

use porter_infer::{Declined, DeclinedBecause, Door, InferRefusal, ModelRef, ProviderId, ServedBy};
use porter_infer::{StageRole, Why};
use serde::{Deserialize, Serialize};

/// One stage of an answer and how its model was reached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteNote {
    /// What the stage did.
    pub stage: StageRole,
    /// Who ran it.
    pub served: ServedBy,
    /// Why that model, in the order inferd said it. Empty when `ai.auto.show_reason` is off,
    /// except for an eviction, which is always said.
    pub why: Vec<WhyWord>,
    /// The provider a hosted model was reached through; none for a model on this computer.
    pub reached: Option<Reached>,
}

/// One reason, a fact and never a judgement (inferd's `Why` without its `Reached`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WhyWord {
    /// The person named it.
    Named,
    /// The only one that can do the request.
    OnlyOne,
    /// It runs on this computer or the network, and the others do not.
    Nearest,
    /// Already loaded.
    Warm,
    /// Cheaper to load than the others.
    Smallest,
    /// Cheaper to use than the others.
    LeastCost,
    /// Nothing told them apart; the catalogue lists it first.
    CatalogueOrder,
    /// The named model could not serve and the person allowed a fallback.
    FallbackFrom {
        /// The model that could not serve.
        model: ModelRef,
    },
    /// Loading it unloaded this idle model.
    Evicted {
        /// The model that was unloaded.
        model: ModelRef,
    },
}

/// A hosted model's provider and door.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reached {
    /// The provider's id (`openrouter`, `anthropic`).
    pub provider: ProviderId,
    /// The model's own company, or a gateway.
    pub door: Door,
}

/// What an inferd `Why` says: a reason, or how the model is reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhySays {
    /// A reason.
    Word(WhyWord),
    /// A door.
    Door(Reached),
}

/// Reads one inferd `Why`.
pub fn why_says(why: &Why) -> WhySays {
    match why {
        Why::Named => WhySays::Word(WhyWord::Named),
        Why::OnlyOne => WhySays::Word(WhyWord::OnlyOne),
        Why::Nearest => WhySays::Word(WhyWord::Nearest),
        Why::Warm => WhySays::Word(WhyWord::Warm),
        Why::Smallest => WhySays::Word(WhyWord::Smallest),
        Why::LeastCost => WhySays::Word(WhyWord::LeastCost),
        Why::CatalogueOrder => WhySays::Word(WhyWord::CatalogueOrder),
        Why::FallbackFrom { model } => WhySays::Word(WhyWord::FallbackFrom {
            model: model.clone(),
        }),
        Why::Evicted { model } => WhySays::Word(WhyWord::Evicted {
            model: model.clone(),
        }),
        Why::Reached { provider, door } => WhySays::Door(Reached {
            provider: provider.clone(),
            door: *door,
        }),
    }
}

/// "Claude Haiku 4.5" from `claude-haiku-4-5`: dashes become spaces, words are capitalised and
/// runs of digits join with dots. A path prefix (`anthropic/`) is dropped.
pub fn model_name(id: &str) -> String {
    let id = id.rsplit('/').next().unwrap_or(id);
    let mut out = String::new();
    let mut prev_digits = false;
    for part in id.split(['-', '_']).filter(|p| !p.is_empty()) {
        let digits = part.chars().all(|c| c.is_ascii_digit());
        out.push(if digits && prev_digits { '.' } else { ' ' });
        prev_digits = digits;
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out.trim().to_owned()
}

/// "OpenRouter" from `openrouter`: the known names, else the id capitalised.
pub fn provider_name(provider: &ProviderId) -> String {
    match provider.0.as_str() {
        "openrouter" => "OpenRouter".to_owned(),
        "openai" => "OpenAI".to_owned(),
        "anthropic" => "Anthropic".to_owned(),
        "google" => "Google".to_owned(),
        other => model_name(other),
    }
}

fn verb(stage: StageRole) -> &'static str {
    match stage {
        StageRole::Hear => "Heard",
        StageRole::Describe => "Described",
        StageRole::Answer => "Answered",
        StageRole::Speak => "Spoken",
    }
}

fn why_text(word: &WhyWord) -> String {
    match word {
        WhyWord::Named => "you chose it".to_owned(),
        WhyWord::OnlyOne => "the only one that fits".to_owned(),
        WhyWord::Nearest => "closest to you".to_owned(),
        WhyWord::Warm => "already loaded".to_owned(),
        WhyWord::Smallest => "quickest to load".to_owned(),
        WhyWord::LeastCost => "least cost".to_owned(),
        WhyWord::CatalogueOrder => "first listed".to_owned(),
        WhyWord::FallbackFrom { model } => {
            format!("{} could not answer", model_name(model.model.as_str()))
        }
        WhyWord::Evicted { model } => format!("unloaded {}", model_name(model.model.as_str())),
    }
}

fn stage_text(note: &RouteNote) -> String {
    let mut text = format!(
        "{} by {}",
        verb(note.stage),
        model_name(note.served.model.as_str())
    );
    if let Some(reached) = &note.reached {
        text.push_str(" via ");
        text.push_str(&provider_name(&reached.provider));
    }
    if !note.why.is_empty() {
        let words: Vec<String> = note.why.iter().map(why_text).collect();
        text.push_str(&format!(" ({})", words.join(", ")));
    }
    text
}

/// The footer line of an answer: "Heard by Whisper · Answered by Kimi K2.6 via OpenRouter
/// (already loaded)". Empty when no stage was announced.
pub fn footer_line(notes: &[RouteNote]) -> String {
    notes.iter().map(stage_text).collect::<Vec<_>>().join(" · ")
}

/// What a refusal says when the person named a model that cannot serve: the model and why.
pub fn declined_text(declined: &Declined) -> String {
    let name = model_name(declined.model.model.as_str());
    let reason = match &declined.because {
        DeclinedBecause::NotListed => "no account offers it for this request".to_owned(),
        DeclinedBecause::Blocked { refusal } => blocked_text(*refusal).to_owned(),
        DeclinedBecause::NotInstalled => "it is not installed on this computer".to_owned(),
        DeclinedBecause::Unavailable => "it cannot run right now".to_owned(),
        DeclinedBecause::NoRoom => "it does not fit in memory".to_owned(),
    };
    format!("{name} cannot answer: {reason}.")
}

fn blocked_text(refusal: InferRefusal) -> &'static str {
    match refusal {
        InferRefusal::RequiresCloud(_) => "this data may not leave this computer",
        InferRefusal::Unavailable => "no model is available",
        InferRefusal::NeedsGrant => "it needs your consent first",
        InferRefusal::Denied => "you refused it for this app",
        InferRefusal::OverBudget => "its spend cap is reached",
        InferRefusal::Unsupported => "it does not fit this request",
    }
}
