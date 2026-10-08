//! One call the agent asked of its host, formed and confined, and the router call it becomes.
//! Pure: no seam is touched. The agent's own title for a request is never carried (the sheet
//! draws the manifest's words and the typed arguments, not an agent's prose), and the text of a
//! write or a command's argument vector stays with the host behind a `StageId`.

use super::confine::Care;
use docket_core::{
    AbsPath, CallRequest, ChoiceId, DERIVES_PARAM, Derivation, FILES_READ, FILES_SENSITIVE,
    FILES_WRITE, FileRef, NETWORK_PARAM, NetAccess, Origin, PermissionKind, REPORTED, TERMINAL_RUN,
    TargetValue, Value, acp_agent_action, derives_choice, network_choice,
};
use prov::{Integrity, Label, Labelled, ModelRole, Source};
use std::collections::BTreeSet;

/// A handle the host holds for what is too big or too private for a sheet or a reviewer (the
/// text of a write, the argument vector and environment of a command): the router carries the
/// name and never the thing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StageId(String);

impl StageId {
    /// The stage numbered `n`.
    pub fn numbered(n: u64) -> Self {
        Self(format!("stage-{n}"))
    }

    /// The name the router carries.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The stage `text` names, if it is one.
    pub fn parse(text: &str) -> Option<Self> {
        text.strip_prefix("stage-")
            .and_then(|n| n.parse::<u64>().ok())
            .map(Self::numbered)
    }
}

/// A command and where it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// The literal command line (program and arguments).
    pub line: String,
    /// Where it runs.
    pub cwd: AbsPath,
}

/// What the host knows of a command that the router rules on (R11): whether its arguments derive
/// from what the agent was served, and what network its sandbox has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunFacts {
    /// Whether the arguments derive from what the agent was served.
    pub derives: Derivation,
    /// What network the command's sandbox has.
    pub network: NetAccess,
}

/// A permission request, reduced to what the router can rule on. The agent's own title is not
/// carried: the sheet draws the manifest's words and the typed arguments, not an agent's prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionAsk {
    /// The kind it named.
    pub kind: PermissionKind,
    /// The paths it names, all inside the session's directory (the rest were refused already).
    pub paths: Vec<AbsPath>,
    /// The most care any of them needs.
    pub care: Care,
    /// The command, for an execute.
    pub command: Option<Command>,
    /// The address, for a fetch.
    pub url: Option<String>,
}

impl PermissionAsk {
    /// The kind it is ruled as: a request the router could not scope (an edit that names no
    /// file, an execute with no command, a fetch with no address) or that touches a place that
    /// later runs code is `other`, destructive until classified, which asks every time and never
    /// offers "always".
    pub fn ruled_as(&self) -> PermissionKind {
        match self.kind {
            PermissionKind::Edit | PermissionKind::Move | PermissionKind::Delete
                if self.paths.is_empty() || self.care == Care::Sensitive =>
            {
                PermissionKind::Other
            }
            PermissionKind::Execute if self.command.is_none() => PermissionKind::Other,
            PermissionKind::Fetch if self.url.is_none() => PermissionKind::Other,
            kind => kind,
        }
    }
}

/// One call the agent asked of its host, formed and confined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentCall {
    /// `fs/read_text_file`.
    Read {
        /// The file, inside the session's directory.
        path: AbsPath,
        /// The staged request (the lines asked for).
        stage: StageId,
    },
    /// `fs/write_text_file`: the text is staged with the host.
    Write {
        /// The file, inside the session's directory.
        path: AbsPath,
        /// How much care it needs: a place that later runs code asks every time.
        care: Care,
        /// How many lines the text has.
        lines: u32,
        /// The staged text.
        stage: StageId,
    },
    /// `terminal/create`: the argument vector and environment are staged with the host.
    Run {
        /// The command and where it runs.
        command: Command,
        /// What the host knows of it.
        facts: RunFacts,
        /// The staged request.
        stage: StageId,
    },
    /// `session/request_permission`.
    Permission(PermissionAsk),
    /// A tool call the agent reports having run itself: asked of the router only so that what it
    /// brought in is taken into the session's taint.
    Reported(PermissionKind),
}

/// What the router is told of a value the agent supplied. Every label it gives is overwritten by
/// the router's own for this role (`Voice::Agent`); this is what it would be if it were not.
fn given() -> Label {
    Label {
        integrity: Integrity::Untrusted,
        confidentiality: prov::Confidentiality::Public,
        classes: BTreeSet::new(),
        sources: BTreeSet::from([Source::Model(ModelRole::Planner)]),
    }
}

type Args = std::collections::BTreeMap<docket_core::ParamName, Labelled<Value>>;

fn args(of: impl IntoIterator<Item = (&'static str, Value)>) -> Option<Args> {
    of.into_iter()
        .map(|(name, value)| {
            let name = docket_core::ParamName::parse(name).ok()?;
            Some((
                name,
                Labelled {
                    value,
                    label: given(),
                },
            ))
        })
        .collect()
}

fn files(paths: &[AbsPath]) -> Option<TargetValue> {
    paths
        .iter()
        .map(|p| FileRef::parse(p.as_str()).ok())
        .collect::<Option<Vec<_>>>()
        .map(TargetValue::Files)
}

fn file(path: &AbsPath) -> Option<Value> {
    FileRef::parse(path.as_str()).ok().map(Value::File)
}

fn choice(id: &str) -> Option<Value> {
    ChoiceId::parse(id).ok().map(Value::Choice)
}

fn number(n: u32) -> Value {
    Value::Integer(i64::from(n))
}

impl AgentCall {
    /// The router call this is. `None` only for a path or name the router's grammar refuses.
    pub fn request(&self) -> Option<CallRequest> {
        let (name, target, args) = match self {
            AgentCall::Read { path, stage } => (
                FILES_READ.to_owned(),
                files(std::slice::from_ref(path))?,
                args([("stage", Value::Text(stage.as_str().to_owned()))])?,
            ),
            AgentCall::Write {
                path,
                care,
                lines,
                stage,
            } => (
                match care {
                    Care::Plain => FILES_WRITE,
                    Care::Sensitive => FILES_SENSITIVE,
                }
                .to_owned(),
                files(std::slice::from_ref(path))?,
                args([
                    ("lines", number(*lines)),
                    ("stage", Value::Text(stage.as_str().to_owned())),
                ])?,
            ),
            AgentCall::Run {
                command,
                facts,
                stage,
            } => (
                TERMINAL_RUN.to_owned(),
                TargetValue::Nothing,
                args([
                    ("command", Value::Text(command.line.clone())),
                    ("cwd", file(&command.cwd)?),
                    ("stage", Value::Text(stage.as_str().to_owned())),
                    (DERIVES_PARAM, choice(derives_choice(facts.derives))?),
                    (NETWORK_PARAM, choice(network_choice(facts.network))?),
                ])?,
            ),
            AgentCall::Permission(ask) => permission(ask)?,
            AgentCall::Reported(kind) => (
                REPORTED.to_owned(),
                TargetValue::Nothing,
                args([("what", Value::Text(kind.word().to_owned()))])?,
            ),
        };
        Some(CallRequest {
            action: acp_agent_action(&name)?,
            target,
            args,
            origin: Origin::Companion,
        })
    }
}

fn permission(ask: &PermissionAsk) -> Option<(String, TargetValue, Args)> {
    let kind = ask.ruled_as();
    let name = kind.action_name();
    Some(match (kind, &ask.command, &ask.url) {
        (PermissionKind::Edit | PermissionKind::Move | PermissionKind::Delete, _, _) => {
            (name, files(&ask.paths)?, Args::new())
        }
        (PermissionKind::Execute, Some(command), _) => (
            name,
            TargetValue::Nothing,
            args([
                ("command", Value::Text(command.line.clone())),
                ("cwd", file(&command.cwd)?),
            ])?,
        ),
        (PermissionKind::Fetch, _, Some(url)) => (
            name,
            TargetValue::Nothing,
            args([("url", Value::Url(url.clone()))])?,
        ),
        _ => (name, TargetValue::Nothing, Args::new()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abs(text: &str) -> AbsPath {
        AbsPath::parse(text).expect("path")
    }

    #[test]
    fn a_write_is_a_call_on_its_file_with_the_text_left_with_the_host() {
        let call = AgentCall::Write {
            path: abs("/work/app/a.rs"),
            care: Care::Plain,
            lines: 3,
            stage: StageId::numbered(7),
        };
        let request = call.request().expect("request");
        assert_eq!(request.action.name.as_str(), FILES_WRITE);
        assert_eq!(
            request.target,
            TargetValue::Files(vec![FileRef::parse("/work/app/a.rs").expect("file")])
        );
        let names: Vec<&str> = request.args.keys().map(|k| k.as_str()).collect();
        assert_eq!(names, ["lines", "stage"]);
        assert_eq!(StageId::parse("stage-7"), Some(StageId::numbered(7)));
        assert_eq!(StageId::parse("7"), None);
        let sensitive = AgentCall::Write {
            path: abs("/work/app/.git/hooks/x"),
            care: Care::Sensitive,
            lines: 1,
            stage: StageId::numbered(8),
        };
        let asked = sensitive.request().expect("request");
        assert_eq!(asked.action.name.as_str(), FILES_SENSITIVE);
    }

    #[test]
    fn a_permission_the_router_cannot_scope_is_ruled_as_other() {
        let ask = |kind, paths: Vec<AbsPath>, command, url| PermissionAsk {
            kind,
            paths,
            care: Care::Plain,
            command,
            url,
        };
        let cases = [
            (
                ask(PermissionKind::Edit, vec![], None, None),
                PermissionKind::Other,
            ),
            (
                ask(PermissionKind::Edit, vec![abs("/work/app/a")], None, None),
                PermissionKind::Edit,
            ),
            (
                ask(PermissionKind::Execute, vec![], None, None),
                PermissionKind::Other,
            ),
            (
                ask(PermissionKind::Fetch, vec![], None, None),
                PermissionKind::Other,
            ),
            (
                ask(
                    PermissionKind::Fetch,
                    vec![],
                    None,
                    Some("https://a.test/".into()),
                ),
                PermissionKind::Fetch,
            ),
            (
                ask(PermissionKind::Think, vec![], None, None),
                PermissionKind::Think,
            ),
            (
                PermissionAsk {
                    care: Care::Sensitive,
                    ..ask(
                        PermissionKind::Edit,
                        vec![abs("/work/app/.git/hooks/x")],
                        None,
                        None,
                    )
                },
                PermissionKind::Other,
            ),
        ];
        for (ask, want) in cases {
            assert_eq!(ask.ruled_as(), want, "{ask:?}");
            let request = AgentCall::Permission(ask).request().expect("request");
            assert_eq!(request.action.name.as_str(), want.action_name());
        }
    }

    #[test]
    fn the_agents_own_words_are_not_carried() {
        let run = AgentCall::Run {
            command: Command {
                line: "cargo test".into(),
                cwd: abs("/work/app"),
            },
            facts: RunFacts {
                derives: Derivation::Independent,
                network: NetAccess::Closed,
            },
            stage: StageId::numbered(1),
        };
        let request = run.request().expect("request");
        assert_eq!(request.target, TargetValue::Nothing);
        let names: Vec<&str> = request.args.keys().map(|k| k.as_str()).collect();
        assert_eq!(names, ["command", "cwd", "derives", "network", "stage"]);
    }
}
