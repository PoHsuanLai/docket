//! The `agy` preset: Google's agy takes its permission rules from a settings file, so docket
//! writes one for each run.
//!
//! agy asks the client's permission (`session/request_permission`) before every tool of its own,
//! its calls to the desktop's tool server included, and the router then gates the real call: two
//! sheets for one action. The file allows the desktop's server and nothing else:
//! `permissions.allow = ["mcp(quire/*)"]`. There are no `ask` rules: a tool no rule names is
//! asked about already (that is the behaviour that was observed), and the asks reach the person
//! through the host's own permission path, which keeps agy's command, file and subagent tools
//! gated. A rule we guessed at the syntax of could only widen or break that.
//!
//! The file is written to a private directory of the run, never into the person's own
//! `.gemini`, and mounted read-only over `$HOME/.gemini/antigravity-acp/settings.json` inside the
//! sandbox (`docket_shell::Overlay`). So it is rewritten from the entry on every run, whatever
//! `state` holds: the person's own file is shadowed, not merged and not changed, and the agent
//! cannot widen the rules. The only trace on the host is an empty placeholder file if the
//! person's state has no `settings.json` yet (a mount needs a target).

use crate::config::Profile;
use docket_acp::client::{SERVER_NAME, SignIn};
use serde_json::{Value, json};

/// Where agy reads its settings, under the sandbox's home.
pub const SETTINGS_AT: &str = ".gemini/antigravity-acp/settings.json";

/// A file a preset has docket write for the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileFile {
    /// The path under the sandbox's home.
    pub at: &'static str,
    /// The contents.
    pub text: String,
}

/// The allow rule for every tool of the desktop's server and no other.
pub fn allow_rule() -> String {
    format!("mcp({SERVER_NAME}/*)")
}

/// The settings: the allow rule, and the sign-in method when the entry names one.
pub fn settings(sign_in: Option<&SignIn>) -> Value {
    let mut out = json!({ "permissions": { "allow": [allow_rule()] } });
    if let Some(method) = sign_in {
        out["auth"] = json!({ "type": method.as_str() });
    }
    out
}

impl Profile {
    /// The file the preset has written for the run, if it uses one.
    pub fn file(self, sign_in: Option<&SignIn>) -> Option<ProfileFile> {
        match self {
            Profile::Agy => Some(ProfileFile {
                at: SETTINGS_AT,
                text: settings(sign_in).to_string(),
            }),
            Profile::ClaudeCode => None,
        }
    }
}
