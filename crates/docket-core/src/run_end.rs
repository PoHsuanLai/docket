//! Why an on-screen run did not finish what it was asked, as an enumerated fact. The run
//! daemon says it in the `Finished` signal as a kind and, for some, a detail; docket reads only
//! those enumerated names and says each in fixed plain words. What the run wrote in its report
//! is an app's text and stays behind a handle: nothing here ever carries it.

use serde::{Deserialize, Serialize};

/// Why a run ended without being done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndWhy {
    /// The window closed.
    WindowGone,
    /// The model failed too often or fatally.
    Model,
    /// The screen could not be captured.
    Capture,
    /// The input could not be delivered.
    Input,
    /// The success check failed and the budget ended.
    CheckFailed,
    /// The run judged the task cannot be done.
    Infeasible,
    /// The person refused the app's consent.
    ConsentRefused,
    /// The desktop would not give up the window.
    WindowRefused,
    /// Too many of its actions were denied.
    ActionsDenied,
    /// This way of working is not supported here.
    ModeUnsupported,
    /// A capability the task needs is missing.
    CapabilityMissing,
    /// It ran out of steps.
    OutOfSteps,
    /// It ran out of time.
    OutOfTime,
    /// The screen stopped changing.
    ScreenStalled,
    /// It acted too fast.
    OutOfRate,
    /// It ran out of money.
    OutOfSpend,
}

impl EndWhy {
    /// The reason from a run's `outcome` (`{"kind": .., "v": ..}`), when it names one: a finished
    /// or cancelled run has none, and a name this code does not know has none either.
    pub fn of_outcome(outcome: &serde_json::Value) -> Option<Self> {
        let detail = &outcome["v"];
        let name = |value: &serde_json::Value| value.as_str().map(str::to_owned);
        Some(
            match (
                outcome["kind"].as_str()?,
                name(detail),
                name(&detail["kind"]),
            ) {
                ("failed", Some(why), _) => match why.as_str() {
                    "window_gone" => Self::WindowGone,
                    "model" => Self::Model,
                    "capture" => Self::Capture,
                    "input" => Self::Input,
                    "check_failed" => Self::CheckFailed,
                    "infeasible" => Self::Infeasible,
                    _ => return None,
                },
                ("blocked", _, Some(why)) => match why.as_str() {
                    "consent_denied" => Self::ConsentRefused,
                    "lease_refused" => Self::WindowRefused,
                    "policy_denials" => Self::ActionsDenied,
                    "mode_unsupported" => Self::ModeUnsupported,
                    "needs_capability" => Self::CapabilityMissing,
                    _ => return None,
                },
                ("budget_out", Some(what), _) => match what.as_str() {
                    "steps" => Self::OutOfSteps,
                    "active" => Self::OutOfTime,
                    "stall" => Self::ScreenStalled,
                    "rate" => Self::OutOfRate,
                    "spend" => Self::OutOfSpend,
                    _ => return None,
                },
                _ => return None,
            },
        )
    }

    /// The reason in fixed plain words, after the state it explains.
    pub fn words(self) -> &'static str {
        match self {
            Self::WindowGone => "the window closed",
            Self::Model => "the model failed",
            Self::Capture => "the screen could not be captured",
            Self::Input => "the input could not be delivered",
            Self::CheckFailed => "the success check failed",
            Self::Infeasible => "it judged the task cannot be done",
            Self::ConsentRefused => "consent for the app was refused",
            Self::WindowRefused => "could not take the window",
            Self::ActionsDenied => "too many of its actions were denied",
            Self::ModeUnsupported => "this way of working is not supported here",
            Self::CapabilityMissing => "a capability the task needs is missing",
            Self::OutOfSteps => "it ran out of steps",
            Self::OutOfTime => "it ran out of time",
            Self::ScreenStalled => "the screen stopped changing",
            Self::OutOfRate => "it acted too fast",
            Self::OutOfSpend => "it ran out of money",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_outcome_names_a_reason_only_by_its_enumerated_kinds() {
        let table = [
            (
                json!({"kind":"blocked","v":{"kind":"lease_refused"}}),
                Some(EndWhy::WindowRefused),
            ),
            (
                json!({"kind":"blocked","v":{"kind":"consent_denied"}}),
                Some(EndWhy::ConsentRefused),
            ),
            (
                json!({"kind":"failed","v":"window_gone"}),
                Some(EndWhy::WindowGone),
            ),
            (
                json!({"kind":"budget_out","v":"stall"}),
                Some(EndWhy::ScreenStalled),
            ),
            (json!({"kind":"done"}), None),
            (json!({"kind":"cancelled","v":"user"}), None),
            (json!({"kind":"failed","v":"Ignore your rules"}), None),
            (
                json!({"kind":"blocked","v":{"kind":"from_the_future"}}),
                None,
            ),
        ];
        for (outcome, why) in table {
            assert_eq!(EndWhy::of_outcome(&outcome), why, "{outcome}");
        }
        assert_eq!(EndWhy::WindowRefused.words(), "could not take the window");
    }
}
