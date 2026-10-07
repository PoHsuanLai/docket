//! `Control`: halting, resuming, the journal and the terminal's standing grants.

use super::{Gateway, json};
use docket_core::{IntentsReply, IntentsRequest};
use docket_dbus::IntentsError;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;

pub(crate) struct ControlBus(pub(crate) Gateway);

#[zbus::interface(name = "org.quire.Intents1.Control")]
impl ControlBus {
    async fn halt(
        &self,
        scope: String,
        cause: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), IntentsError> {
        let request = IntentsRequest::ControlHalt {
            scope: json(&scope)?,
            cause: json(&cause)?,
        };
        self.0.done(&header, request).await?;
        let _ = Self::halted(&emitter, &scope).await;
        Ok(())
    }

    async fn resume(
        &self,
        scope: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), IntentsError> {
        let request = IntentsRequest::ControlResume {
            scope: json(&scope)?,
        };
        self.0.done(&header, request).await?;
        let _ = Self::resumed(&emitter, &scope).await;
        Ok(())
    }

    async fn state(&self, #[zbus(header)] header: Header<'_>) -> Result<String, IntentsError> {
        self.0
            .answer(&header, IntentsRequest::ControlState, |r| match r {
                IntentsReply::State(kill) => Some(kill),
                _ => None,
            })
            .await
    }

    async fn journal(
        &self,
        filter: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, IntentsError> {
        self.0
            .answer(
                &header,
                IntentsRequest::ControlJournal(json(&filter)?),
                |r| match r {
                    IntentsReply::Journal(rows) => Some(rows),
                    _ => None,
                },
            )
            .await
    }

    async fn terminal_grants(
        &self,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, IntentsError> {
        self.0
            .answer(
                &header,
                IntentsRequest::ControlTerminalGrants,
                |r| match r {
                    IntentsReply::TerminalGrants(actions) => Some(actions),
                    _ => None,
                },
            )
            .await
    }

    async fn revoke_terminal_grant(
        &self,
        action: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), IntentsError> {
        self.0
            .done(
                &header,
                IntentsRequest::ControlTerminalRevoke(json(&action)?),
            )
            .await
    }

    async fn standing_grants(
        &self,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, IntentsError> {
        self.0
            .answer(
                &header,
                IntentsRequest::ControlStandingGrants,
                |r| match r {
                    IntentsReply::StandingGrants(grants) => Some(grants),
                    _ => None,
                },
            )
            .await
    }

    async fn revoke_standing_grant(
        &self,
        id: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), IntentsError> {
        self.0
            .done(&header, IntentsRequest::ControlStandingRevoke(json(&id)?))
            .await
    }

    #[zbus(signal)]
    async fn halted(emitter: &SignalEmitter<'_>, scope: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn resumed(emitter: &SignalEmitter<'_>, scope: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn journal_changed(emitter: &SignalEmitter<'_>, rows: u64) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn breaker_tripped(emitter: &SignalEmitter<'_>, session: &str) -> zbus::Result<()>;
}
