//! What an external agent's calls are called in our records: `acp.<program>.<tail>`, in the
//! app `org.quire.Acp`. A call that went through our gate has the tail of its kind (`edit`,
//! `execute`); one the agent only reported has `reported.<kind>` and is never dispatched.

use agent_client_protocol_schema::v1::ToolKind;
use docket_core::ActionRef;
use docket_session::ProgramName;
use porter_core::AppName;
use prov::{ActionName, Effect};

/// The app that owns every agent action.
pub const ACP_APP: &str = "org.quire.Acp";

/// The program as one lower-case action-name element.
fn element(program: &ProgramName) -> String {
    let mut out: String = program
        .as_str()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if !out.starts_with(|c: char| c.is_ascii_lowercase()) {
        out.insert(0, 'p');
    }
    out
}

/// `acp.<program>.<tail>`. `tail` is a fixed word or dotted words from this module.
pub fn action(program: &ProgramName, tail: &str) -> Option<ActionRef> {
    Some(ActionRef {
        app: AppName::parse(ACP_APP).ok()?,
        name: ActionName::parse(&format!("acp.{}.{tail}", element(program))).ok()?,
    })
}

/// The tail for an agent's tool kind.
pub fn tail(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Read => "read",
        ToolKind::Edit => "edit",
        ToolKind::Delete => "delete",
        ToolKind::Move => "move",
        ToolKind::Search => "search",
        ToolKind::Execute => "execute",
        ToolKind::Think => "think",
        ToolKind::Fetch => "fetch",
        ToolKind::SwitchMode => "switch_mode",
        _ => "other",
    }
}

/// The tail for a call the agent only reported.
pub fn reported_tail(kind: ToolKind) -> String {
    format!("reported.{}", tail(kind))
}

/// The effect a kind is gated as: read and search read; edit and move write (undoable, we keep
/// the old text); delete is destructive; execute is `EXECUTE_AS`; fetch goes out; a mode switch
/// and anything unknown are destructive until classified (they never offer "always").
pub fn effect(kind: ToolKind) -> Effect {
    match kind {
        ToolKind::Read | ToolKind::Search | ToolKind::Think => Effect::Read,
        ToolKind::Edit | ToolKind::Move => Effect::UndoableWrite,
        ToolKind::Execute | ToolKind::Fetch => Effect::Outbound,
        ToolKind::Delete | ToolKind::SwitchMode => Effect::Destructive,
        _ => Effect::Destructive,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_name_becomes_one_element_and_every_kind_has_an_effect() {
        let program = ProgramName::parse("Claude Code").expect("program");
        let call = action(&program, "edit").expect("action");
        assert_eq!(call.name.as_str(), "acp.claude_code.edit");
        let odd = ProgramName::parse("9lives").expect("program");
        assert_eq!(
            action(&odd, "read").expect("action").name.as_str(),
            "acp.p9lives.read"
        );
        assert_eq!(effect(ToolKind::Delete), Effect::Destructive);
        assert_eq!(effect(ToolKind::SwitchMode), Effect::Destructive);
        assert_eq!(effect(ToolKind::Execute), Effect::Outbound);
        assert_eq!(effect(ToolKind::Fetch), Effect::Outbound);
        assert_eq!(effect(ToolKind::Other), Effect::Destructive);
        assert_eq!(effect(ToolKind::Read), Effect::Read);
        assert_eq!(effect(ToolKind::Edit), Effect::UndoableWrite);
    }
}
