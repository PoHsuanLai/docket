//! `Session`: one per task; the role decides who may call what (the router checks).

use super::{Gateway, json, parsed};
use docket_core::{Handle, IntentsReply, IntentsRequest, NoteAsk, ReadAsk, TurnId, WidenAsk};
use docket_dbus::{Details, IntentsError};
use prov::SessionId;
use zbus::message::Header;
use zbus::zvariant::OwnedObjectPath;

pub(crate) struct SessionBus(pub(crate) Gateway);

fn session_of(text: &str) -> Result<SessionId, IntentsError> {
    parsed(text, SessionId::parse)
}

#[zbus::interface(name = "org.quire.Intents1.Session")]
impl SessionBus {
    async fn open(
        &self,
        open: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, IntentsError> {
        self.0
            .answer(
                &header,
                IntentsRequest::SessionOpen(json(&open)?),
                |r| match r {
                    IntentsReply::SessionOpened(o) => Some(o),
                    _ => None,
                },
            )
            .await
    }

    async fn turn(
        &self,
        session: String,
        turn: String,
        options: Details,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<u64, IntentsError> {
        let _ = options;
        let request = IntentsRequest::SessionTurn {
            session: session_of(&session)?,
            turn: json(&turn)?,
        };
        self.0
            .ask(&header, request, |r| match r {
                IntentsReply::TurnRecorded(TurnId(id)) => Some(id),
                _ => None,
            })
            .await
    }

    async fn close(
        &self,
        session: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), IntentsError> {
        let request = IntentsRequest::SessionClose {
            session: session_of(&session)?,
        };
        self.0.done(&header, request).await
    }

    async fn resolve(
        &self,
        session: String,
        handle: u64,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, IntentsError> {
        let request = IntentsRequest::SessionResolve {
            session: session_of(&session)?,
            handle: Handle(handle),
        };
        self.0.ask(&header, request, text).await
    }

    async fn display(
        &self,
        session: String,
        handle: u64,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, IntentsError> {
        let request = IntentsRequest::SessionDisplay {
            session: session_of(&session)?,
            handle: Handle(handle),
        };
        self.0.ask(&header, request, text).await
    }

    async fn read(
        &self,
        session: String,
        ask: String,
        options: Details,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, IntentsError> {
        let _ = options;
        let request = IntentsRequest::SessionRead {
            session: session_of(&session)?,
            ask: ReadAsk { ask: json(&ask)? },
        };
        self.0
            .answer(&header, request, |r| match r {
                IntentsReply::Read(value) => Some(value),
                _ => None,
            })
            .await
    }

    async fn task_policy(
        &self,
        session: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, IntentsError> {
        let request = IntentsRequest::SessionTaskPolicy {
            session: session_of(&session)?,
        };
        self.0
            .answer(&header, request, |r| match r {
                IntentsReply::TaskPolicy(policy) => Some(policy.map(|p| *p)),
                _ => None,
            })
            .await
    }

    async fn widen(
        &self,
        session: String,
        turn: u64,
        change: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> Result<OwnedObjectPath, IntentsError> {
        let request = IntentsRequest::SessionWiden {
            session: session_of(&session)?,
            widen: WidenAsk {
                turn: TurnId(turn),
                change: json(&change)?,
            },
        };
        self.0.start(&header, connection, request).await
    }

    async fn note(
        &self,
        session: String,
        episode: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), IntentsError> {
        let note: NoteAsk = json(&episode)?;
        let request = IntentsRequest::SessionNote {
            session: session_of(&session)?,
            note,
        };
        self.0.done(&header, request).await
    }

    async fn recall(
        &self,
        session: String,
        ask: String,
        options: Details,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, IntentsError> {
        let _ = options;
        let request = IntentsRequest::SessionRecall {
            session: session_of(&session)?,
            ask: json(&ask)?,
        };
        self.0
            .answer(&header, request, |r| match r {
                IntentsReply::Recalled(view) => Some(view),
                _ => None,
            })
            .await
    }
}

fn text(reply: IntentsReply) -> Option<String> {
    match reply {
        IntentsReply::Text(text) => Some(text),
        _ => None,
    }
}
