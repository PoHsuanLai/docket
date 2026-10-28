//! A conversation with an agent app moving between docket and the app's own terminal program.
//! The session stays one session at the router: an agent session while docket runs the app, a
//! watch session (restore points only, marked by the terminal) while the person works in the
//! terminal, and an agent session again when the conversation is brought back. A terminal that
//! watches an agent app it can name opens its watch session the same way, so the same step
//! brings it into docket. Every move is a `Hand` note in the session's log.

use crate::router::Router;
use crate::seams::Seams;
use crate::session::SessionState;
use crate::state::SessionKind;
use companion_model::{HandNote, HandTo, WatchedAgent, WatchedLine};
use docket_core::{ExternalAgent, Rewind, SheetSurface, TurnState};
use docket_session::SessionEntry;
use docket_wire::{CallerId, IntentsReply, WireRefusal};
use prov::SessionId;

impl<S: Seams> Router<S> {
    /// Names the agent app of a watch session a terminal just opened and the app's own id for
    /// the conversation, so the session can be brought into docket.
    pub(crate) fn name_watched_agent(&self, session: &SessionId, agent: WatchedAgent) {
        let mut st = self.locked();
        let Some(record) = st.sessions.get_mut(session) else {
            return;
        };
        let rewind = match record.kind {
            SessionKind::Watch(rewind) => rewind,
            SessionKind::Agent => Rewind::default(),
        };
        record.external = Some(ExternalAgent {
            program: agent.program.clone(),
            sheets: SheetSurface::Desktop,
            label: record.label.clone(),
            rewind,
            helper: None,
        });
        record.agent_session = Some(agent.session.clone());
        record.wal.note(SessionEntry::Hand(HandNote {
            to: HandTo::Terminal,
            agent: Some(agent),
        }));
    }

    /// `.Checkpoint.Handover`: moves the conversation of `session`. To the terminal, the host
    /// that opened the session says which app and which conversation of its own; back, the router
    /// answers them so the host can start the app on that conversation.
    pub(crate) fn checkpoint_handover(
        &self,
        caller: &CallerId,
        session: &SessionId,
        to: HandTo,
        agent: Option<WatchedAgent>,
    ) -> IntentsReply {
        // A turn that is running is the agent's: nothing moves under it.
        if self.turn_state(session) == TurnState::Running {
            return IntentsReply::Refused(WireRefusal::NotAllowed);
        }
        let mut st = self.locked();
        let Some(record) = st.sessions.get_mut(session) else {
            return IntentsReply::Refused(WireRefusal::NoSuchSession);
        };
        if !matches!(record.state, SessionState::Open(_)) {
            return IntentsReply::Refused(WireRefusal::NotAllowed);
        }
        let named = match (to, record.kind) {
            (HandTo::Terminal, SessionKind::Agent) => {
                let (Some(external), Some(agent)) = (record.external.clone(), agent) else {
                    return IntentsReply::Refused(WireRefusal::Malformed);
                };
                if record.opener != caller.app.name || external.program != agent.program {
                    return IntentsReply::Refused(WireRefusal::NotAllowed);
                }
                record.kind = SessionKind::Watch(external.rewind);
                record.label = external.label;
                record.agent_session = Some(agent.session.clone());
                agent
            }
            (HandTo::Desktop, SessionKind::Watch(_)) => {
                let (Some(external), Some(own)) =
                    (record.external.as_ref(), record.agent_session.take())
                else {
                    return IntentsReply::Refused(WireRefusal::NotAllowed);
                };
                let program = external.program.clone();
                record.kind = SessionKind::Agent;
                record.label = None;
                WatchedAgent {
                    program,
                    session: own,
                }
            }
            _ => return IntentsReply::Refused(WireRefusal::NotAllowed),
        };
        record.wal.note(SessionEntry::Hand(HandNote {
            to,
            agent: Some(named.clone()),
        }));
        IntentsReply::HandedOver(Some(named))
    }

    /// `.Checkpoint.Watched`: the open watch sessions whose agent app is named.
    pub(crate) fn checkpoint_watched(&self) -> IntentsReply {
        let st = self.locked();
        let lines = st
            .sessions
            .iter()
            .filter_map(|(id, record)| {
                let open = matches!(record.state, SessionState::Open(_));
                let watch = matches!(record.kind, SessionKind::Watch(_));
                if !(open && watch) {
                    return None;
                }
                Some(WatchedLine {
                    session: id.clone(),
                    agent: WatchedAgent {
                        program: record.external.as_ref()?.program.clone(),
                        session: record.agent_session.clone()?,
                    },
                    label: record.label.clone(),
                    folder: record.cwd.clone(),
                })
            })
            .collect();
        IntentsReply::Watched(lines)
    }
}
