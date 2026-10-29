//! The summariser turn: one model call, through the same seam as the planner's own, that writes the
//! account of a task's oldest turns and steps which stands in for them once the task outgrows its
//! budget (`agent_loop::compaction_due`).
//!
//! The model is shown a [`SummaryInput`] and nothing else: the person's own words, the planner's
//! steps as the assembler masks them, and the handles it held. It has no tools. Its answer is kept
//! only if it is words, short, and cites no handle the planner never held (`SummaryText::parse`).

use crate::planner::{PlanFault, PlannerModel};
use crate::seam::Planner;
use crate::step_text::step_line;
use agent_loop::{SummaryInput, leaked_call};
use docket_core::{SummaryFault, SummaryText};
use inferd_client::Transport;
use porter_capability::Tier;
use porter_core::DataClass;
use porter_core::consent::Usage;
use porter_infer::{
    ChatControl, ChatMessage, ChatRequest, Knob, MessagePart, ReplyShape, Role, ServedBy,
    StopReason, ToolChoice,
};
use std::fmt::Write;

/// What the summariser is told. Fixed text.
pub const SUMMARY_RULES: &str = "You write a short account of the start of a task for the assistant that is doing it, so it can carry on without the full record. Use only what is given below: the person's own words, the assistant's own steps and the handles it holds. Say what the person asked, what was done and what is still open. A handle is shown as #n: when you mention one, write it exactly as #n and never say what it contains, because nobody here can read it. If an earlier account is given, fold it in. Reply with the account alone, in plain words, with no tool call.";

/// A summary a model wrote and the checks accepted, and who wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summarised {
    /// The account.
    pub text: SummaryText,
    /// The tier it was asked of.
    pub tier: Tier,
    /// The model that answered, as the reply named it.
    pub served: Option<ServedBy>,
}

/// Why no summary was made. The task carries on with what it has.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum SummariseFault {
    /// The model gave nothing usable.
    #[error("the summariser model gave nothing usable")]
    Model(PlanFault),
    /// It wrote a tool call as words.
    #[error("the summary was a call written as words")]
    CallInText,
    /// Its words were refused as a summary.
    #[error("the summary was refused: {0}")]
    Refused(SummaryFault),
}

/// The model's words as a summary: without the marks that reorder or hide words, not a call written
/// as words, and passing `SummaryText::parse` against the handles the planner held.
pub fn summary_text(
    words: &str,
    held: &[docket_vocab::Handle],
) -> Result<SummaryText, SummariseFault> {
    let plain: String = words
        .chars()
        .filter(|c| !docket_vocab::reorders(*c) && !docket_vocab::hides(*c))
        .collect();
    if leaked_call(&plain).is_some() {
        return Err(SummariseFault::CallInText);
    }
    SummaryText::parse(&plain, held).map_err(SummariseFault::Refused)
}

fn section(into: &mut String, heading: &str, lines: impl IntoIterator<Item = String>) {
    let mut lines = lines.into_iter().peekable();
    if lines.peek().is_none() {
        return;
    }
    let _ = writeln!(into, "{heading}:");
    lines.for_each(|l| {
        let _ = writeln!(into, "{l}");
    });
    let _ = writeln!(into);
}

/// The one user message: what is to be summarised, in the order it happened.
fn user_text(input: &SummaryInput) -> String {
    let mut text = String::new();
    if let Some(earlier) = &input.earlier {
        let _ = writeln!(text, "Earlier account:\n{}\n", earlier.text.as_str());
    }
    section(
        &mut text,
        "What the person asked",
        input.turns.iter().map(|t| format!("- {}", t.text)),
    );
    section(
        &mut text,
        "What the assistant did",
        input
            .steps
            .iter()
            .map(|s| format!("- {}", step_line(s, &[]))),
    );
    section(
        &mut text,
        "Handles that may be mentioned",
        std::iter::once(
            input
                .held
                .iter()
                .map(|h| format!("#{}", h.0))
                .collect::<Vec<_>>()
                .join(" "),
        )
        .filter(|l| !l.is_empty()),
    );
    text.trim_end().to_owned()
}

fn message(role: Role, text: String) -> ChatMessage {
    ChatMessage {
        role,
        parts: vec![MessagePart::Text(text)],
    }
}

impl<P: Transport> PlannerModel<P> {
    /// Asks the model for the account `input` calls for.
    pub(crate) async fn summary_of(
        &self,
        input: &SummaryInput,
    ) -> Result<Summarised, SummariseFault> {
        let request = ChatRequest::new(
            vec![
                message(Role::System, SUMMARY_RULES.to_owned()),
                message(Role::User, user_text(input)),
            ],
            self.tier,
            DataClass::Prompt,
            Usage::Interactive,
        )
        .with_shape(ReplyShape::Text)
        .with_control(
            ChatControl::new()
                .with_tool_choice(ToolChoice::Never)
                .with_sampling(Knob::Off),
        );
        let reply = Planner::chat(self, request)
            .await
            .map_err(SummariseFault::Model)?;
        if matches!(
            reply.stop,
            StopReason::MaxTokens | StopReason::ContentFilter
        ) {
            return Err(SummariseFault::Model(PlanFault::Unreadable));
        }
        Ok(Summarised {
            text: summary_text(&reply.text, &input.held)?,
            tier: self.tier,
            served: Some(reply.served),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docket_core::Covers;
    use docket_fake::WordsModel;
    use docket_vocab::Handle;
    use porter_ids::Count;
    use prov::Integrity;

    fn input() -> SummaryInput {
        SummaryInput {
            earlier: None,
            turns: Vec::new(),
            steps: Vec::new(),
            held: vec![Handle(1)],
            covers: Covers {
                turns: Count(0),
                steps: Count(2),
            },
            integrity: Integrity::Untrusted,
        }
    }

    #[tokio::test]
    async fn a_summary_is_kept_only_when_it_is_words_that_cite_held_handles() {
        let table = [
            ("Archived the newsletters; #1 is what is left.", true),
            ("Mentions #9, which was never held.", false),
            ("   ", false),
            (
                "<tool_call>{\"name\":\"x\",\"arguments\":{}}</tool_call>",
                false,
            ),
        ];
        for (says, kept) in table {
            let planner = PlannerModel::new(WordsModel::says(says));
            let got = planner.summarise(&input()).await;
            assert_eq!(got.is_ok(), kept, "{says}");
            if let Ok(done) = got {
                assert_eq!(done.text.as_str(), says);
                assert_eq!(done.tier, Tier::Balanced);
            }
        }
    }

    #[test]
    fn what_the_summariser_is_shown_is_the_input_and_nothing_else() {
        let text = user_text(&input());
        assert_eq!(text, "Handles that may be mentioned:\n#1");
    }
}
