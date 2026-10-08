//! The lines spoken between the bridge process an external ACP agent starts (`actions-mcp
//! --host-socket`) and the host that started the agent (`docket-acp`'s per-session edge).
//!
//! A request names a token the host minted for one session and an operation, nothing else: there
//! is no field for a session, a program or an app, so nothing the agent writes can name one. The
//! host knows which session a socket serves because it made the socket for that session. Unknown
//! fields are refused. A reply to a bad token is one word and says nothing of why.

use crate::fault::McpFault;
use crate::tools::McpTool;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

/// The environment variable that names the socket in the MCP server entry the host offers.
pub const EDGE_SOCKET_ENV: &str = "QUIRE_EDGE_SOCKET";
/// The environment variable that carries the token in the MCP server entry the host offers.
pub const EDGE_TOKEN_ENV: &str = "QUIRE_EDGE_TOKEN";
/// The longest line either side reads (a tool call's arguments or a listing).
pub const EDGE_LINE_MAX: usize = 4 * 1024 * 1024;

/// The capability one session's edge accepts: 64 hex characters the host minted. It is never
/// printed: `Debug` hides it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EdgeToken(String);

impl EdgeToken {
    /// A token from its text; `None` unless it is 64 lowercase hex characters.
    pub fn parse(text: &str) -> Option<Self> {
        let hex = text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        hex.then(|| Self(text.to_owned()))
    }

    /// The text, for the environment of the process that is to use it.
    pub fn reveal(&self) -> &str {
        &self.0
    }

    /// Whether `other` is the same token, compared without stopping at the first difference.
    pub fn matches(&self, other: &EdgeToken) -> bool {
        self.0.len() == other.0.len()
            && self
                .0
                .bytes()
                .zip(other.0.bytes())
                .fold(0_u8, |diff, (a, b)| diff | (a ^ b))
                == 0
    }
}

impl std::fmt::Debug for EdgeToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EdgeToken(..)")
    }
}

/// What the bridge asks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum EdgeOp {
    /// The tools offered now.
    List,
    /// One tool call; the arguments are the JSON the agent gave.
    Call {
        /// The tool's name.
        tool: String,
        /// The arguments, an object or null.
        arguments: Json,
    },
}

/// One line from the bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeRequest {
    /// The capability.
    pub token: EdgeToken,
    /// The operation.
    pub op: EdgeOp,
}

/// One tool and its description.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeTool {
    /// The tool.
    pub tool: McpTool,
    /// What it does, in the action's label.
    pub description: String,
}

/// One line back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeReply {
    /// The tools offered.
    Tools(Vec<EdgeTool>),
    /// The call's outcome, as `outcome_json` gives it.
    Done(Json),
    /// The call gave no outcome.
    Failed(McpFault),
    /// The line was not a request, or the token is not this session's. Nothing was done.
    Refused,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token() -> EdgeToken {
        EdgeToken::parse(&"ab".repeat(32)).expect("token")
    }

    #[test]
    fn a_token_is_sixty_four_hex_characters_and_never_printed() {
        assert!(EdgeToken::parse("abc").is_none());
        assert!(EdgeToken::parse(&"AB".repeat(32)).is_none());
        assert!(!format!("{:?}", token()).contains("abab"));
    }

    #[test]
    fn tokens_compare_by_content() {
        let other = EdgeToken::parse(&"cd".repeat(32)).expect("token");
        assert!(token().matches(&token()));
        assert!(!token().matches(&other));
    }

    #[test]
    fn a_request_names_no_session_program_or_app() {
        let line = serde_json::json!({
            "token": token().reveal(),
            "op": {"call": {"tool": "t", "arguments": null}},
            "session": "s-9",
        });
        assert!(serde_json::from_value::<EdgeRequest>(line).is_err());
        let line = serde_json::json!({
            "token": token().reveal(),
            "op": {"call": {"tool": "t", "arguments": null, "app": "x"}},
        });
        assert!(serde_json::from_value::<EdgeRequest>(line).is_err());
    }

    #[test]
    fn a_request_round_trips() {
        let request = EdgeRequest {
            token: token(),
            op: EdgeOp::Call {
                tool: "mail__send".into(),
                arguments: serde_json::json!({"to": "a"}),
            },
        };
        let text = serde_json::to_string(&request).expect("json");
        assert_eq!(
            serde_json::from_str::<EdgeRequest>(&text).expect("back"),
            request
        );
    }
}
