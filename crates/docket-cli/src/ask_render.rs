//! `quire-do ask`: an answer as lines for a terminal. Pure: the answer in, text out. A handle is
//! printed as `#<n>` (the text it names was held for the companion, and a terminal may not have
//! it displayed), the footer says which model answered and where it ran.

use companion_wire::{
    AnswerBody, AnswerPhase, AnswerWire, FooterWire, NeedsYou, PlanWire, RefusalWire, StepWireState,
};
use docket_core::Reveal;
use porter_core::Locality;

fn reveal(text: &Reveal<String>) -> String {
    match text {
        Reveal::Plain(text) => text.clone(),
        Reveal::Handle(handle) => format!("#{}", handle.0),
    }
}

fn state(state: &StepWireState) -> &'static str {
    match state {
        StepWireState::Pending => "pending",
        StepWireState::Running => "running",
        StepWireState::Done { .. } => "done",
        StepWireState::Failed(_) => "failed",
        StepWireState::Skipped => "skipped",
        StepWireState::Undone => "undone",
    }
}

/// The plan's steps, one per line, as `  1. done     Label (effect)`.
pub fn plan_lines(plan: &PlanWire) -> Vec<String> {
    plan.steps
        .iter()
        .enumerate()
        .map(|(n, step)| {
            format!(
                "  {}. {:<8} {} ({})",
                n + 1,
                state(&step.state),
                step.label,
                step.effect.slug()
            )
        })
        .collect()
}

fn refusal(refusal: &RefusalWire) -> String {
    match refusal {
        RefusalWire::NeedsCloud(class) => format!(
            "this needs a cloud model and the data class {class:?} may not leave this computer"
        ),
        RefusalWire::NotAllowed(code) => format!("not allowed: {code:?}"),
        RefusalWire::NoWay { app } => format!("no way to do that in {app}"),
        RefusalWire::OverBudget(kind) => format!("over budget: {kind:?}"),
        RefusalWire::Failed(why) => format!("failed: {why}"),
    }
}

fn body(body: &AnswerBody) -> Vec<String> {
    match body {
        AnswerBody::Text { lines } => lines.iter().map(reveal).collect(),
        AnswerBody::Plan(plan) => ["Plan".to_owned()]
            .into_iter()
            .chain(plan_lines(plan))
            .collect(),
        AnswerBody::DraftReply { subject, body, .. } => vec![
            format!("Draft reply, subject: {}", reveal(subject)),
            reveal(body),
        ],
        AnswerBody::ProposedEvent { title, .. } => vec![format!("Proposed event: {title}")],
        AnswerBody::Replace { proposed, .. } => vec![format!("Proposed: {}", reveal(proposed))],
        AnswerBody::Form(form) => vec![format!("Needs more for {}", form.action.name)],
        AnswerBody::Refused(why) => vec![refusal(why)],
    }
}

/// What the person is needed for. A confirmation is answered in the shell, never here.
pub fn needs_you(needs: &NeedsYou) -> Vec<String> {
    match needs {
        NeedsYou::Confirm(id) => vec![
            format!("Waiting for your confirmation ({id})."),
            "Confirmations are answered in the shell (sill's sheet); quire-do cannot answer them."
                .to_owned(),
        ],
        NeedsYou::Question { text, choices } => {
            let mut lines = vec![format!("The companion asks: {text}")];
            lines.extend(choices.iter().map(|c| format!("  - {c}")));
            lines
        }
        NeedsYou::Form(form) => vec![format!(
            "The companion needs more for {}.",
            form.action.name
        )],
    }
}

fn place(locality: &Locality) -> &'static str {
    match locality {
        Locality::OnDevice => "on this computer",
        Locality::LocalNetwork => "on your network",
        Locality::Cloud { .. } => "in the cloud",
    }
}

/// The footer line: who answered, where, and what it drew on; empty when nothing did.
pub fn footer(footer: &FooterWire) -> Option<String> {
    let served: Vec<String> = footer
        .served
        .iter()
        .map(|s| format!("{}/{} {}", s.account, s.model, place(&s.locality)))
        .collect();
    let mut parts = Vec::new();
    if !served.is_empty() {
        parts.push(format!("answered by {}", served.join(", ")));
    }
    if !footer.sources.is_empty() {
        parts.push(format!("{} sources", footer.sources.len()));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// The whole answer for a terminal.
pub fn human(answer: &AnswerWire) -> String {
    let mut lines = body(&answer.body);
    match &answer.phase {
        AnswerPhase::NeedsYou(needs) => lines.extend(needs_you(needs)),
        AnswerPhase::Failed => lines.push("The companion could not finish.".to_owned()),
        AnswerPhase::Cancelled => lines.push("Stopped.".to_owned()),
        AnswerPhase::Thinking | AnswerPhase::Streaming | AnswerPhase::Done => {}
    }
    if let Some(line) = footer(&answer.footer) {
        lines.push(String::new());
        lines.push(line);
    }
    lines.join("\n")
}
