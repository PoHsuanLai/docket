//! A permission request, read. The agent's `toolCall` says a kind, a title, the paths it touches
//! and its raw input; none of it is trusted. This reduces it to what the router can rule on: the
//! kind, the paths (each confined as `fs/*` paths are), a command line for an execute, an address
//! for a fetch. A path outside the session's directory or one that holds secrets makes the whole
//! request `Forbidden`: it is refused without asking, before any call is formed. The agent's
//! title is dropped: it is prose, and the sheet does not draw an agent's prose.

use super::call::{Command, PermissionAsk};
use super::confine::{Care, confine, named};
use super::names;
use agent_client_protocol_schema::v1::{ToolCallUpdate, ToolKind};
use docket_core::AbsPath;
use serde_json::Value;

/// A permission request, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// It names a place the agent may not reach: refused, nobody asked.
    Forbidden,
    /// What the router rules on.
    Ask(PermissionAsk),
}

/// The command line in a tool's raw input: a string `command`, or a list of words.
fn command_line(raw: &Value) -> Option<String> {
    match raw.get("command")? {
        Value::String(line) if !line.trim().is_empty() => Some(line.clone()),
        Value::Array(words) => {
            let words: Option<Vec<&str>> = words.iter().map(Value::as_str).collect();
            words.map(|w| w.join(" ")).filter(|l| !l.trim().is_empty())
        }
        _ => None,
    }
}

/// The address in a tool's raw input `url`: an `http` or `https` URL with a host and no user
/// information, as written.
fn address(raw: &Value) -> Option<String> {
    let url = raw.get("url")?.as_str()?;
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    (!authority.is_empty() && !authority.contains('@') && url.len() <= 2048).then(|| url.to_owned())
}

/// Reads `update`, working in `scope` (whose links resolve to `real_scope`).
pub fn tool_req(
    scope: &AbsPath,
    real_scope: &AbsPath,
    real: &dyn Fn(&AbsPath) -> Option<AbsPath>,
    update: &ToolCallUpdate,
) -> Asked {
    let fields = &update.fields;
    let kind = names::permission(fields.kind.unwrap_or(ToolKind::Other));
    let mut paths = Vec::new();
    let mut care = Care::Plain;
    for location in fields.locations.iter().flatten() {
        let confined = location
            .path
            .to_str()
            .and_then(|text| named(scope, text).ok())
            .and_then(|path| {
                let at = real(&path)?;
                confine(real_scope, path, at).ok()
            });
        match confined {
            Some(c) => {
                if c.care == Care::Sensitive {
                    care = Care::Sensitive;
                }
                paths.push(c.path);
            }
            None => return Asked::Forbidden,
        }
    }
    let raw = fields.raw_input.clone().unwrap_or(Value::Null);
    let command = command_line(&raw).map(|line| Command {
        line,
        cwd: scope.clone(),
    });
    Asked::Ask(PermissionAsk {
        kind,
        paths,
        care,
        command,
        url: address(&raw),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_is_a_plain_http_url_and_nothing_else() {
        let url = |u: &str| address(&serde_json::json!({ "url": u }));
        assert_eq!(
            url("https://Example.org/a?b").as_deref(),
            Some("https://Example.org/a?b")
        );
        assert!(url("http://user:pw@example.org:8080/x").is_none());
        assert!(url("file:///etc/passwd").is_none());
        assert!(url("example.org").is_none());
        assert!(address(&serde_json::json!({})).is_none());
    }

    #[test]
    fn a_command_is_a_string_or_a_list_of_words() {
        let c = |v: Value| command_line(&serde_json::json!({ "command": v }));
        assert_eq!(c(serde_json::json!("ls -la")).as_deref(), Some("ls -la"));
        assert_eq!(
            c(serde_json::json!(["cargo", "test"])).as_deref(),
            Some("cargo test")
        );
        assert!(c(serde_json::json!("  ")).is_none());
        assert!(c(serde_json::json!(5)).is_none());
    }
}
