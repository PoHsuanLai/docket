//! Which part of the stack a model exchange belongs to, from the exchange alone: the policy
//! writer, a review stage, the planner, or the reader. A tap line says who asked and what; this
//! reads the rest from the words, because the reviewer's and the writer's instructions are fixed
//! text (`action-review`'s render, `intentd`'s writer).

use docket_core::{ModelExchange, Stage};

/// What asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asker {
    /// The task-policy writer.
    Writer,
    /// One stage of the reviewer cascade.
    Review(Stage),
    /// The companion's planner (it offers tools).
    Planner,
    /// The quarantined reader or anything else.
    Other,
}

/// The words that open each fixed instruction.
const WRITER: &str = "You write a task policy";
const SECOND: &str = "You are an independent second opinion";
const REVIEWING: &str = "You review one action";

/// Who asked, from the request.
pub fn asker(exchange: &ModelExchange) -> Asker {
    let text = exchange.sent_text();
    match () {
        () if !exchange.tools.is_empty() => Asker::Planner,
        () if text.contains(WRITER) => Asker::Writer,
        () if text.contains(REVIEWING) && text.contains(SECOND) => {
            Asker::Review(Stage::SecondOpinion)
        }
        () if text.contains(REVIEWING) && exchange.shape == "choice" => Asker::Review(Stage::Quick),
        () if text.contains(REVIEWING) => Asker::Review(Stage::Deliberate),
        () => Asker::Other,
    }
}
