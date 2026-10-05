//! `Run` and `Gate`: the members that act, most of them through a Request object.

use super::request::Watching;
use super::{Gateway, json, parsed};
use docket_core::{GrantAsk, IntentsReply, IntentsRequest, UndoId};
use docket_dbus::{Details, IntentsError};
use zbus::message::Header;
use zbus::zvariant::OwnedObjectPath;

pub(crate) struct RunBus(pub(crate) Gateway);

#[zbus::interface(name = "org.quire.Intents1.Run")]
impl RunBus {
    async fn perform(
        &self,
        call: String,
        session: String,
        parent_window: String,
        options: Details,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> Result<OwnedObjectPath, IntentsError> {
        // A caller that watches is told how far the call is (`Reviewing`, `Previewing`,
        // `Confirming`, `Dispatched`); nothing waits for it.
        let watching = match options.contains_key(docket_dbus::OPTION_WATCH) {
            true => Watching::Listening,
            false => Watching::No,
        };
        let request = IntentsRequest::Perform {
            call: json(&call)?,
            session: json(&session)?,
            parent_window: json(&parent_window)?,
        };
        self.0
            .start_as(&header, connection, request, watching)
            .await
    }

    async fn dry_run(
        &self,
        call: String,
        session: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, IntentsError> {
        let request = IntentsRequest::DryRun {
            call: json(&call)?,
            session: json(&session)?,
        };
        self.0
            .answer(&header, request, |reply| {
                super::or_refused(reply, |r| match r {
                    IntentsReply::Preview(p) => Some(p),
                    _ => None,
                })
            })
            .await
    }

    async fn preview(
        &self,
        entity: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, IntentsError> {
        self.0
            .answer(&header, IntentsRequest::Preview(json(&entity)?), |reply| {
                super::or_refused(reply, |r| match r {
                    IntentsReply::Preview(p) => Some(p),
                    _ => None,
                })
            })
            .await
    }

    async fn suggest(
        &self,
        ask: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, IntentsError> {
        self.0
            .answer(&header, IntentsRequest::Suggest(json(&ask)?), |reply| {
                super::or_refused(reply, |r| match r {
                    IntentsReply::Suggestions(s) => Some(s),
                    _ => None,
                })
            })
            .await
    }

    async fn undo(
        &self,
        entry: u64,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> Result<OwnedObjectPath, IntentsError> {
        self.0
            .start(&header, connection, IntentsRequest::Undo(UndoId(entry)))
            .await
    }

    async fn undo_all(
        &self,
        scope: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> Result<OwnedObjectPath, IntentsError> {
        self.0
            .start(&header, connection, IntentsRequest::UndoAll(json(&scope)?))
            .await
    }
}

pub(crate) struct GateBus(pub(crate) Gateway);

#[zbus::interface(name = "org.quire.Intents1.Gate")]
impl GateBus {
    async fn grant(
        &self,
        app: String,
        space: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> Result<OwnedObjectPath, IntentsError> {
        let ask = GrantAsk {
            app: parsed(&app, porter_core::AppName::parse)?,
            space: parsed(&space, prov::SpaceId::parse)?,
        };
        self.0
            .start(&header, connection, IntentsRequest::GateGrant(ask))
            .await
    }

    async fn check(
        &self,
        ask: String,
        options: Details,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> Result<OwnedObjectPath, IntentsError> {
        let watching = match options.contains_key(docket_dbus::OPTION_WATCH) {
            true => Watching::Yes,
            false => Watching::No,
        };
        self.0
            .start_as(
                &header,
                connection,
                IntentsRequest::GateCheck(json(&ask)?),
                watching,
            )
            .await
    }
}
