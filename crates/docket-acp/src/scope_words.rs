//! What "allow always" would cover, in words. Every word comes from the typed scope the router
//! offered and the manifest's label for the action: no model text, no argument the router did not
//! validate. A path, a command prefix and an address have already passed their own parsers, which
//! refuse control characters and shell syntax.

use docket_core::{Recipient, StandingScope};

fn to(recipient: &Recipient) -> String {
    match recipient {
        Recipient::Address(address) => address.clone(),
        Recipient::Domain(domain) => format!("anyone at {}", domain.as_str()),
    }
}

/// The option's name for `scope`, for the action the sheet calls `label`.
pub fn always_words(label: &str, scope: &StandingScope) -> String {
    match scope {
        StandingScope::Files { under, .. } => {
            format!("Always allow \"{label}\" on files under {}", under.as_str())
        }
        StandingScope::Terminal { command, cwd, .. } => format!(
            "Always allow \"{label}\" for commands starting \"{}\" in {} and below",
            command.as_text(),
            cwd.as_str()
        ),
        StandingScope::Outbound { to: whom, .. } => {
            format!("Always allow \"{label}\" to {}", to(whom))
        }
        StandingScope::Reads { .. } => format!("Always allow \"{label}\" (read only)"),
    }
}
