//! The modes the editor may pick. They shape only the editor's own prompt, which sits on top of
//! our gate: no mode skips the router's policy, reviewer, taint, breaker or budgets, and none
//! gives a right the Space's strictness withholds.

use agent_client_protocol_schema::v1::{SessionMode, SessionModeId, SessionModeState};
use docket_session::CallOpen;
use prov::Effect;

/// A mode of the session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Mode {
    /// The editor is asked before any call that changes something (the default).
    #[default]
    Ask,
    /// A call that changes something is stopped; reads go on.
    ReadOnly,
    /// The editor is not asked; our gate alone decides, as on the desktop.
    AutoJudged,
}

/// What the editor's side of the gate does with one call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Say {
    /// Nothing to ask.
    Proceed,
    /// Ask the editor's person.
    Ask,
    /// Stop it here.
    Stop,
}

impl Mode {
    /// Every mode, in the order they are offered.
    pub const ALL: [Mode; 3] = [Mode::Ask, Mode::ReadOnly, Mode::AutoJudged];

    /// The mode's id on the wire.
    pub fn id(self) -> &'static str {
        match self {
            Mode::Ask => "ask",
            Mode::ReadOnly => "read-only",
            Mode::AutoJudged => "auto-judged",
        }
    }

    /// The mode named `id`.
    pub fn parse(id: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|m| m.id() == id)
    }

    fn describe(self) -> (&'static str, &'static str) {
        match self {
            Mode::Ask => ("Ask", "Ask before any call that changes something."),
            Mode::ReadOnly => ("Read only", "Look, never change anything."),
            Mode::AutoJudged => (
                "Auto-judged",
                "No extra prompt here; the assistant's own checks still run.",
            ),
        }
    }

    /// What this mode says about `call`.
    pub fn says(self, call: &CallOpen) -> Say {
        match (self, call.effect) {
            (Mode::AutoJudged, _) | (_, Effect::Read) => Say::Proceed,
            (Mode::Ask, _) => Say::Ask,
            (Mode::ReadOnly, _) => Say::Stop,
        }
    }

    /// The modes as the protocol lists them, this one current.
    pub fn state(self) -> SessionModeState {
        let all = Mode::ALL
            .into_iter()
            .map(|m| {
                let (name, about) = m.describe();
                SessionMode::new(SessionModeId::new(m.id()), name).description(about.to_owned())
            })
            .collect();
        SessionModeState::new(SessionModeId::new(self.id()), all)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_unknown_ones_are_none() {
        for m in Mode::ALL {
            assert_eq!(Mode::parse(m.id()), Some(m));
        }
        assert_eq!(Mode::parse("yolo"), None);
        assert_eq!(Mode::default(), Mode::Ask);
    }
}
