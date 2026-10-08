//! What the host tells the router about one terminal command, and how the standing-grant rule
//! reads it (acp-sessions.md section 12, R11). The host alone knows what it served and what
//! sandbox the command runs in; the router alone rules. The two facts travel as the `derives`
//! and `network` choice parameters of `acpagent.terminal.run`.
//!
//! A value the host did not send, or one it sent that is not a known choice, is read the
//! conservative way: derived, with the network open. So a missing fact can only cost a question.

use crate::exec_derive::Derivation;
use crate::exec_reach::{NetAccess, NetReach};
use crate::review::AskReason;
use serde::{Deserialize, Serialize};

/// The parameter that says whether the arguments derive from what was served.
pub const DERIVES_PARAM: &str = "derives";
/// The parameter that says what network the command's sandbox has.
pub const NETWORK_PARAM: &str = "network";

/// The choice ids of [`DERIVES_PARAM`].
pub const DERIVES_OWN: &str = "own";
/// See [`DERIVES_OWN`].
pub const DERIVES_READ: &str = "read";
/// The choice ids of [`NETWORK_PARAM`].
pub const NETWORK_CLOSED: &str = "closed";
/// See [`NETWORK_CLOSED`].
pub const NETWORK_OPEN: &str = "open";

/// The choice id the host sends for `derivation`.
pub fn derives_choice(derivation: Derivation) -> &'static str {
    match derivation {
        Derivation::Independent => DERIVES_OWN,
        Derivation::Derived => DERIVES_READ,
    }
}

/// The choice id the host sends for `access`.
pub fn network_choice(access: NetAccess) -> &'static str {
    match access {
        NetAccess::Closed => NETWORK_CLOSED,
        NetAccess::Open => NETWORK_OPEN,
    }
}

/// What the grant rule reads about a terminal command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExecFacts {
    /// Whether its arguments derive from what the agent read.
    pub derivation: Derivation,
    /// Whether it can send data out.
    pub reach: NetReach,
}

impl ExecFacts {
    /// The facts for `line` from the two choices the host sent.
    pub fn from_choices(derives: Option<&str>, network: Option<&str>, line: &str) -> Self {
        let derivation = match derives {
            Some(DERIVES_OWN) => Derivation::Independent,
            _ => Derivation::Derived,
        };
        let access = match network {
            Some(NETWORK_CLOSED) => NetAccess::Closed,
            _ => NetAccess::Open,
        };
        Self {
            derivation,
            reach: NetReach::of(line, access),
        }
    }

    /// Whether the only untrusted thing about this command is that the session read something:
    /// nothing in it comes from what was read, and it cannot send anything out. The session's
    /// taint then says nothing about this command (the Rule of Two needs a channel out; an
    /// untrusted sink needs an untrusted argument).
    pub fn is_session_taint_only(&self) -> bool {
        self.derivation == Derivation::Independent && self.reach == NetReach::None
    }

    /// Whether `reason` is the session's taint and nothing more.
    pub fn excuses(&self, reason: &AskReason) -> bool {
        self.is_session_taint_only()
            && matches!(
                reason,
                AskReason::Tainted | AskReason::RuleOfTwo | AskReason::UntrustedSink(_)
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ArgSink;

    #[test]
    fn missing_or_unknown_facts_are_read_the_conservative_way() {
        let own = ExecFacts::from_choices(Some("own"), Some("closed"), "cargo test");
        assert_eq!(
            (own.derivation, own.reach),
            (Derivation::Independent, NetReach::None)
        );
        for (derives, network) in [
            (None, Some("closed")),
            (Some("own"), None),
            (Some("maybe"), Some("closed")),
            (Some("own"), Some("endpoint")),
        ] {
            let f = ExecFacts::from_choices(derives, network, "cargo test");
            assert!(!f.is_session_taint_only(), "{derives:?} {network:?}");
        }
    }

    #[test]
    fn only_a_command_of_its_own_with_no_way_out_excuses_the_sessions_taint() {
        let reasons = [
            AskReason::Tainted,
            AskReason::RuleOfTwo,
            AskReason::UntrustedSink(ArgSink::Body),
        ];
        let own = ExecFacts::from_choices(Some("own"), Some("closed"), "ls");
        let derived = ExecFacts::from_choices(Some("read"), Some("closed"), "ls");
        let out = ExecFacts::from_choices(Some("own"), Some("closed"), "curl a.test");
        for r in &reasons {
            assert!(
                own.excuses(r) && !derived.excuses(r) && !out.excuses(r),
                "{r:?}"
            );
        }
        assert!(!own.excuses(&AskReason::OutsideTask));
        assert!(!own.excuses(&AskReason::AskAlways));
    }
}
