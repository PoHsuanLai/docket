//! The seam between the host of an external agent and the router. Every call the agent asks of
//! its host (a file read or write, a command, a permission request) is made through it as an
//! ordinary router call of the `org.quire.AcpAgent` pseudo-app, and the host performs it only
//! after the router has allowed it. The host decides nothing: not whether to ask the person, not
//! whether a grant stands in, not whether the breaker is tripped.
//!
//! What the host does before a call is form it: a path is confined to the session's directory
//! (links and secrets included) before the call exists, because a path outside is not a call
//! the router should be asked to weigh.

use super::confine::Care;
use docket_core::{
    AbsPath, CallRefusal, CallRequest, ExternalAgent, FILES_READ, FILES_SENSITIVE, FILES_WRITE,
    FileRef, Origin, Outcome, PermissionKind, REPORTED, SheetSurface, TERMINAL_RUN, TargetValue,
    Value, acp_agent_action,
};
use docket_session::{ProgramName, Workspace};
use prov::{Integrity, Label, Labelled, ModelRole, SessionId, Source};
use std::collections::BTreeSet;
use std::future::Future;

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
            AgentCall::Run { command, stage } => (
                TERMINAL_RUN.to_owned(),
                TargetValue::Nothing,
                args([
                    ("command", Value::Text(command.line.clone())),
                    ("cwd", file(&command.cwd)?),
                    ("stage", Value::Text(stage.as_str().to_owned())),
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

/// How the router ruled on a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ruled {
    /// It went ahead, and this is what the host performed (a read's text, a terminal's name).
    Done(Box<Outcome>),
    /// It did not.
    Refused(CallRefusal),
    /// The router could not be asked or did not answer: nothing was done.
    Lost,
}

/// Why the router would not open or follow a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CourtFault {
    /// The router refused the request.
    #[error("the router refused")]
    Refused,
    /// The router could not be reached.
    #[error("the router could not be reached")]
    Unavailable,
}

/// What the host tells the router when it opens an agent's session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAgent {
    /// The configured program, as the host launched it.
    pub program: ProgramName,
    /// The directory the agent works in.
    pub cwd: Workspace,
    /// Where the person answers its sheets.
    pub sheets: SheetSurface,
}

impl OpenAgent {
    /// The agent the router is told of.
    pub fn external(&self) -> ExternalAgent {
        ExternalAgent {
            program: self.program.clone(),
            sheets: self.sheets,
        }
    }
}

/// The router, as the host of an external agent sees it. `Clone` because the host and the
/// backend each hold one.
///
/// **Cancel safety.** The backend may drop the future of `call` and ask again with the same `n`
/// (the call's number in this connection); an implementation must then go on waiting for the
/// one call and make no second one, or the person would see two sheets and the command would run
/// twice.
pub trait Court: Send + Clone + 'static {
    /// Opens the agent's session.
    fn open(
        &mut self,
        open: OpenAgent,
    ) -> impl Future<Output = Result<SessionId, CourtFault>> + Send;

    /// Records the person's turn, from which the task policy derives. May wait for the person
    /// (a turn that widens the task asks).
    fn turn(
        &mut self,
        session: &SessionId,
        text: &str,
    ) -> impl Future<Output = Result<(), CourtFault>> + Send;

    /// Makes one call as the router rules.
    fn call(
        &mut self,
        session: &SessionId,
        n: u64,
        call: &AgentCall,
    ) -> impl Future<Output = Ruled> + Send;

    /// Ends the session.
    fn close(&mut self, session: &SessionId) -> impl Future<Output = ()> + Send;
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
            stage: StageId::numbered(1),
        };
        let request = run.request().expect("request");
        assert_eq!(request.target, TargetValue::Nothing);
        let names: Vec<&str> = request.args.keys().map(|k| k.as_str()).collect();
        assert_eq!(names, ["command", "cwd", "stage"]);
    }
}
