//! Two things that end a hold the person can no longer end: the time (a mic is never held longer
//! than `max_hold_ms`) and the shell (the connection that began the utterance left the bus).

use futures_util::{Stream, StreamExt};
use std::future::pending;
use std::pin::Pin;
use std::time::Duration;
use tokio::time::Instant;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;

/// How long the mic may stay open when `voiced.toml` says nothing (`max_hold_ms`).
pub(crate) const DEFAULT_MAX_HOLD_MS: u64 = 60_000;

/// The latest moment the mic may still be open.
#[derive(Debug, Default)]
pub(crate) struct Deadline(Option<Instant>);

impl Deadline {
    /// A deadline `max` from now.
    pub(crate) fn after(max: Duration) -> Self {
        Self(Some(Instant::now() + max))
    }

    /// Resolves when the deadline passes, and then forgets it. Never, when none is set.
    pub(crate) async fn expired(&mut self) {
        match self.0 {
            Some(at) => {
                tokio::time::sleep_until(at).await;
                self.0 = None;
            }
            None => pending().await,
        }
    }
}

type Departure = Pin<Box<dyn Stream<Item = ()> + Send>>;

/// The departure of the connection that began the utterance, watched from before the mic opens.
#[derive(Default)]
pub(crate) struct Leaving(Option<Departure>);

impl Leaving {
    /// Watches the unique name `unique`. A name that already left (or a bus that cannot be
    /// asked) counts as departed, so the hold ends at once rather than going unwatched.
    pub(crate) async fn of(connection: &zbus::Connection, unique: &str) -> Self {
        let gone = || -> Departure { Box::pin(futures_util::stream::once(std::future::ready(()))) };
        let Ok(dbus) = DBusProxy::new(connection).await else {
            return Self(Some(gone()));
        };
        let Ok(stream) = dbus
            .receive_name_owner_changed_with_args(&[(0, unique), (2, "")])
            .await
        else {
            return Self(Some(gone()));
        };
        let Ok(name) = BusName::try_from(unique) else {
            return Self(Some(gone()));
        };
        match dbus.name_has_owner(name).await {
            Ok(true) => Self(Some(Box::pin(stream.map(|_| ())))),
            _ => Self(Some(gone())),
        }
    }

    /// Resolves when the caller leaves the bus, and then forgets it. Never, when none is
    /// watched.
    pub(crate) async fn left(&mut self) {
        match self.0.as_mut() {
            Some(stream) => {
                stream.next().await;
                self.0 = None;
            }
            None => pending().await,
        }
    }
}
