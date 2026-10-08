//! Who a request acts for. The connection says which role called; the router says which
//! session that role acts in, and how far to believe the labels it supplies.

use crate::labels::Voice;
use crate::state::{RouterState, SessionRecord};
use docket_core::{CallerId, CallerRole, GrantCaller, TurnSource, WireRefusal};
use prov::{Actor, AgentRole, ClientName, SessionId, TaskId};

/// The party behind a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Who {
    /// The connection.
    pub caller: CallerId,
    /// The role it acts in.
    pub role: CallerRole,
    /// The session it acts in; none for the person, whose own surfaces are not agents.
    pub session: Option<SessionId>,
    /// How calls and the journal name it.
    pub actor: Actor,
    /// How far its labels are believed.
    pub voice: Voice,
    /// The editor the person is speaking through, named by the app behind its connection (what
    /// the router saw, never a name the editor wrote about itself).
    pub editor: Option<ClientName>,
}

/// The editor the person last spoke through in `record`: the app that recorded its latest turn,
/// if that turn came from an editor.
pub(crate) fn editor_of(record: &SessionRecord) -> Option<ClientName> {
    match record.turns.last().map(|t| &t.from) {
        Some(TurnSource::Editor(app)) => ClientName::parse(app.as_str()).ok(),
        _ => None,
    }
}

/// Where a sheet for a call of `who` goes when an editor is driving its session.
pub(crate) fn route_of(who: &Who) -> Option<docket_core::EditorRoute> {
    Some(docket_core::EditorRoute {
        client: who.editor.clone()?,
        session: who.session.clone()?,
    })
}

impl Who {
    /// The consent a grant for this party is keyed by.
    pub(crate) fn grant_caller(&self) -> GrantCaller {
        if let Some(client) = &self.editor
            && !matches!(
                self.actor,
                Actor::Companion {
                    role: AgentRole::Cua { .. },
                    ..
                }
            )
        {
            return GrantCaller::Editor(client.clone());
        }
        match &self.actor {
            Actor::Companion {
                role: AgentRole::Cua { .. },
                ..
            } => GrantCaller::Cua,
            Actor::Mcp { client } => GrantCaller::Mcp(client.clone()),
            Actor::Cli => GrantCaller::Cli,
            Actor::App { app } | Actor::ThirdParty { app, .. } => GrantCaller::App(app.clone()),
            Actor::Companion { .. }
            | Actor::User { .. }
            | Actor::System { .. }
            | Actor::Unknown => GrantCaller::Companion,
        }
    }
}

impl RouterState {
    /// Who a `role` calling as `caller` is. The person's surfaces act directly; a companion
    /// acts in the session it names, else its newest (a closed one refuses its calls, so a halt reads as a halt); MCP clients, terminals and apps act in a session of
    /// their own, made when they first call.
    pub(crate) fn who_for(
        &mut self,
        caller: &CallerId,
        role: CallerRole,
        named: Option<&SessionId>,
        now: prov::UnixSeconds,
    ) -> Result<Who, WireRefusal> {
        let app = caller.app.name.clone();
        let (actor, voice) = match role {
            CallerRole::Launcher | CallerRole::Field | CallerRole::Editor => {
                let actor = Actor::User { via: app };
                let editor = (role == CallerRole::Editor)
                    .then(|| ClientName::parse(caller.app.name.as_str()).ok())
                    .flatten();
                return Ok(Who {
                    caller: caller.clone(),
                    role,
                    session: None,
                    actor,
                    voice: Voice::Person,
                    editor,
                });
            }
            CallerRole::Companion => {
                let mut companions = self
                    .sessions
                    .iter()
                    .filter(|(_, r)| matches!(r.actor, Actor::Companion { .. }));
                let session = match named {
                    Some(wanted) => companions.find(|(id, _)| *id == wanted),
                    None => companions.max_by_key(|(id, _)| crate::messaging::age(id)),
                }
                .map(|(id, r)| (id.clone(), r.actor.clone(), editor_of(r)))
                .ok_or(WireRefusal::NoSuchSession)?;
                return Ok(Who {
                    caller: caller.clone(),
                    role,
                    session: Some(session.0),
                    actor: session.1,
                    voice: Voice::Model,
                    editor: session.2,
                });
            }
            CallerRole::Mcp => {
                let client = ClientName::parse(app.as_str()).map_err(|_| WireRefusal::Malformed)?;
                (
                    Actor::Mcp {
                        client: client.clone(),
                    },
                    Voice::External(client),
                )
            }
            CallerRole::Cli => (Actor::Cli, Voice::Cli),
            CallerRole::App => (Actor::App { app: app.clone() }, Voice::App),
            CallerRole::Reader
            | CallerRole::Cua
            | CallerRole::Confirm
            | CallerRole::Compositor
            | CallerRole::Control => return Err(WireRefusal::NotAllowed),
        };
        let key = (role, app.clone());
        let session = match self
            .implicit
            .get(&key)
            .filter(|s| self.sessions.contains_key(*s))
        {
            Some(existing) => existing.clone(),
            None => {
                let n = self.mint();
                let session =
                    SessionId::parse(&format!("s-{n}")).map_err(|_| WireRefusal::Malformed)?;
                let task = TaskId::parse(&format!("t-{n}")).map_err(|_| WireRefusal::Malformed)?;
                let record =
                    SessionRecord::new(task, actor.clone(), app, prov::SpaceId::desktop(), now);
                self.sessions.insert(session.clone(), record);
                self.implicit.insert(key, session.clone());
                session
            }
        };
        Ok(Who {
            caller: caller.clone(),
            role,
            session: Some(session),
            actor,
            voice,
            editor: None,
        })
    }
}
