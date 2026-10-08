//! The development fallback (`docket-agent --tty`): what the host prints of a turn, and the
//! router's sheet put to the person on this terminal. Without the flag none of this is wired:
//! the sheet is the desktop's, through the router's confirmer, and the host sees nothing of it.
//! What the agent says of itself is printed as the agent's words; the sheet is the router's own
//! request, with its words and typed arguments, never an agent's prose.

use docket_acp::always_words;
use docket_core::{AlwaysOffer, ConfirmRequest, Shown, StepEnd};
use docket_session::{BackendEvent, CallEvent, SheetChoice};

/// A line for a sheet argument: somebody else's words are quoted with where they came from.
fn shown(value: &Shown) -> String {
    match value {
        Shown::Plain(text) => text.clone(),
        Shown::Quoted { text, from } => format!("\"{text}\" (from {from:?})"),
    }
}

/// The sheet as text, and whether "always" is among its answers.
pub fn render(request: &ConfirmRequest) -> (String, bool) {
    let mut out = format!(
        "\n? {} ({:?}, {})",
        request.action.as_str(),
        request.effect,
        request.app
    );
    for line in &request.lines {
        out.push_str(&format!(
            "\n    {}: {}",
            line.label.as_str(),
            shown(&line.value)
        ));
    }
    let offered = match &request.always {
        AlwaysOffer::Offered(scope) => {
            out.push_str(&format!("\n  [a] {}", always_words("this", scope)));
            true
        }
        AlwaysOffer::Withheld(_) => false,
    };
    out.push_str("\n  [y] once, [a] always (when offered), anything else no: ");
    (out, offered)
}

/// What the person typed, as a choice. "Always" where it was not offered is once.
pub fn choice(typed: Option<&str>, always_offered: bool) -> SheetChoice {
    match typed.map(str::trim) {
        Some("y") => SheetChoice::Once,
        Some("a") if always_offered => SheetChoice::Always,
        Some("a") => SheetChoice::Once,
        _ => SheetChoice::Refused,
    }
}

/// One event of the turn, as the person sees it.
pub fn print(event: &BackendEvent) {
    match event {
        BackendEvent::Words(docket_core::Reveal::Plain(text)) => print!("{text}"),
        BackendEvent::Call(CallEvent::Started(open)) => {
            eprintln!("\n> {} ({:?})", open.action.name, open.effect);
        }
        BackendEvent::Call(CallEvent::Ended(step)) => {
            let how = match &step.end {
                StepEnd::Done { .. } => "done",
                StepEnd::Unconfirmed(_) => "not confirmed",
                StepEnd::Interrupted => "interrupted",
                _ => "refused",
            };
            eprintln!("  = {how}");
        }
        BackendEvent::TurnEnd(end) => eprintln!("\n[turn ended: {end:?}]"),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_yes_is_a_yes_and_always_needs_an_offer() {
        assert_eq!(choice(Some("y"), false), SheetChoice::Once);
        assert_eq!(choice(Some("a"), true), SheetChoice::Always);
        assert_eq!(choice(Some("a"), false), SheetChoice::Once);
        assert_eq!(choice(Some("yes please"), true), SheetChoice::Refused);
        assert_eq!(choice(None, true), SheetChoice::Refused);
    }
}
