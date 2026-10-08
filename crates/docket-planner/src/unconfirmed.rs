//! What the planner reads when a call was not confirmed. A no, a dismissal and an expiry are the
//! person's answer, not a fault to route around: the line says so and says what to do, so the
//! model finishes instead of offering the same call again or searching for another way to it.

use docket_core::ConfirmEnd;

/// Why the call did not run, from the planner's side of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cause {
    /// The person said no or closed the question.
    Declined,
    /// The person did not answer in time.
    Expired,
    /// The router withdrew the question (a halt, a cancel).
    Withdrawn,
}

impl Cause {
    fn of(end: ConfirmEnd) -> Self {
        match end {
            ConfirmEnd::Refused | ConfirmEnd::Dismissed => Self::Declined,
            ConfirmEnd::Expired => Self::Expired,
            ConfirmEnd::Cancelled => Self::Withdrawn,
        }
    }

    fn reason(self) -> &'static str {
        match self {
            Self::Declined => "the person declined this",
            Self::Expired => "the person did not answer in time",
            Self::Withdrawn => "the question to the person was withdrawn",
        }
    }
}

const NEXT: &str = "do not retry it, offer to retry it or work around it; finish, saying what was not done. Ask the person with quire_ask only about a different way to what they wanted, never about this call";

/// The words after `not confirmed` on a step line: the reason and what to do next.
pub(crate) fn unconfirmed_text(end: ConfirmEnd) -> String {
    format!("{}; {NEXT}", Cause::of(end).reason())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_decline_is_told_as_the_persons_answer() {
        let text = unconfirmed_text(ConfirmEnd::Refused);
        assert!(
            text.starts_with("the person declined this; do not retry it"),
            "{text}"
        );
        assert!(text.contains("finish, saying what was not done."), "{text}");
        assert!(text.contains("never about this call"), "{text}");
        assert_eq!(text, unconfirmed_text(ConfirmEnd::Dismissed));
    }

    #[test]
    fn an_expiry_says_the_person_did_not_answer_in_time() {
        let text = unconfirmed_text(ConfirmEnd::Expired);
        assert!(
            text.starts_with("the person did not answer in time; do not retry it"),
            "{text}"
        );
        assert_ne!(text, unconfirmed_text(ConfirmEnd::Refused));
    }

    #[test]
    fn a_withdrawn_question_gets_the_same_instruction() {
        let text = unconfirmed_text(ConfirmEnd::Cancelled);
        assert!(text.contains("withdrawn; do not retry it"), "{text}");
    }
}
