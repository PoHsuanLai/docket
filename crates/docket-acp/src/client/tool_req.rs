//! A permission request, read. The agent's `toolCall` says a kind, a title, the paths it touches
//! and its raw input; none of it is trusted. This reduces it to what the gate can compare: the
//! effect from the kind, the paths (each confined as `fs/*` paths are), a command line for an
//! execute, a host for a fetch. A path outside the session's directory or one that holds secrets
//! makes the whole request `forbidden`: it is refused without asking.

use super::ask::Shown;
use super::confine::{confine, named};
use super::files::Files;
use super::gate::ToolReq;
use super::names;
use agent_client_protocol_schema::v1::{ToolCallUpdate, ToolKind};
use docket_core::{AbsPath, ArgFacts, CallFacts, Domain, Recipient};
use docket_session::ProgramName;
use serde_json::Value;

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

/// The host in a tool's raw input `url`.
fn host(raw: &Value) -> Option<Domain> {
    let url = raw.get("url")?.as_str()?;
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?;
    let host = host.rsplit_once(':').map_or(host, |(h, port)| {
        if port.chars().all(|c| c.is_ascii_digit()) {
            h
        } else {
            host
        }
    });
    Domain::parse(host).ok()
}

/// Reads `update` for `program`, working in `scope` (whose links resolve to `real_scope`).
pub fn tool_req(
    program: &ProgramName,
    scope: &AbsPath,
    real_scope: &AbsPath,
    files: &impl Files,
    update: &ToolCallUpdate,
) -> Option<ToolReq> {
    let fields = &update.fields;
    let kind = fields.kind.unwrap_or(ToolKind::Other);
    let mut paths = Vec::new();
    let mut forbidden = false;
    for location in fields.locations.iter().flatten() {
        let ok = location
            .path
            .to_str()
            .and_then(|text| named(scope, text).ok())
            .and_then(|path| {
                let real = files.real(&path).ok()?;
                confine(real_scope, path, real).ok()
            });
        match ok {
            Some(confined) => paths.push(confined.path),
            None => forbidden = true,
        }
    }
    let raw = fields.raw_input.clone().unwrap_or(Value::Null);
    let line = (kind == ToolKind::Execute)
        .then(|| command_line(&raw))
        .flatten();
    let args = match kind {
        ToolKind::Edit | ToolKind::Move | ToolKind::Delete if !paths.is_empty() => {
            ArgFacts::Paths(paths.clone())
        }
        ToolKind::Edit | ToolKind::Move | ToolKind::Delete => ArgFacts::Opaque,
        ToolKind::Execute => match &line {
            Some(line) => ArgFacts::Command {
                line: line.clone(),
                cwd: scope.clone(),
            },
            None => ArgFacts::Opaque,
        },
        ToolKind::Fetch => match host(&raw) {
            Some(domain) => ArgFacts::Recipients(vec![Recipient::Domain(domain)]),
            None => ArgFacts::Opaque,
        },
        _ => ArgFacts::Unscoped,
    };
    let action = names::action(program, names::tail(kind))?;
    Some(ToolReq {
        kind,
        facts: CallFacts { action, args },
        effect: names::effect(kind),
        title: Shown::of(fields.title.as_deref().unwrap_or("")),
        line,
        paths,
        forbidden,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_is_read_from_a_url_and_only_from_a_url() {
        let url = |u: &str| host(&serde_json::json!({ "url": u }));
        assert_eq!(
            url("https://Example.org/a?b")
                .map(|d| d.as_str().to_owned())
                .as_deref(),
            Some("example.org")
        );
        assert_eq!(
            url("http://user:pw@example.org:8080/x")
                .map(|d| d.as_str().to_owned())
                .as_deref(),
            Some("example.org")
        );
        assert!(url("example.org").is_none());
        assert!(host(&serde_json::json!({})).is_none());
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
