//! Watching a request over D-Bus: the Request object's `Progress` and `Response` signals come
//! out in order (from intentd's own connection, at the request's path), and `Proceed` and
//! `Close` are calls on the same object. The signals are subscribed to before the method that
//! starts the request, so nothing quicker than its reply is lost.

use crate::transport::TransportError;
use crate::watch::{Boxed, Events, Said, Steering, Watched};
use docket_core::{CallProgress, IntentsReply};
use docket_dbus::{BusConnection, INTENTS_BUS, RequestProxy};
use futures_util::StreamExt;
use futures_util::future::{Either, select};
use std::future::Future;
use zbus::fdo::DBusProxy;
use zbus::message::Type;
use zbus::zvariant::OwnedObjectPath;
use zbus::{MatchRule, MessageStream};

fn bus<E: std::fmt::Display>(error: E) -> TransportError {
    TransportError::Bus(error.to_string())
}

/// What a `Response` of this request means as a reply: its code and body.
type Reading = fn((u32, String)) -> Result<IntentsReply, TransportError>;

struct Signals {
    messages: MessageStream,
    leaving: zbus::fdo::NameOwnerChangedStream,
    owner: String,
    path: String,
    reading: Reading,
    over: bool,
}

impl Events for Signals {
    fn next(&mut self) -> Boxed<'_, Result<Said, TransportError>> {
        Box::pin(async move {
            if self.over {
                return Err(TransportError::Closed);
            }
            loop {
                let next = select(
                    Box::pin(self.messages.next()),
                    Box::pin(self.leaving.next()),
                )
                .await;
                match next {
                    Either::Left((Some(Ok(message)), _)) => {
                        let header = message.header();
                        let ours = header.sender().map(|s| s.as_str()) == Some(self.owner.as_str())
                            && header.path().map(|p| p.as_str()) == Some(self.path.as_str());
                        if !ours {
                            continue;
                        }
                        match header.member().map(|m| m.as_str()) {
                            Some("Progress") => {
                                let text: String = message.body().deserialize().map_err(bus)?;
                                let progress: CallProgress = serde_json::from_str(&text)
                                    .map_err(|e| TransportError::Malformed(e.to_string()))?;
                                return Ok(Said::Progress(progress));
                            }
                            Some("Response") => {
                                self.over = true;
                                let answer = message.body().deserialize().map_err(bus)?;
                                return (self.reading)(answer).map(Said::Answer);
                            }
                            _ => {}
                        }
                    }
                    Either::Left((Some(Err(error)), _)) => return Err(bus(error)),
                    Either::Left((None, _)) | Either::Right((None, _)) => {
                        return Err(TransportError::Closed);
                    }
                    Either::Right((Some(change), _)) => {
                        let gone = change
                            .args()
                            .map(|a| a.new_owner().is_none())
                            .unwrap_or(true);
                        if gone {
                            return Err(TransportError::Closed);
                        }
                    }
                }
            }
        })
    }
}

struct Calls {
    connection: BusConnection,
    path: OwnedObjectPath,
}

impl Calls {
    async fn proxy(&self) -> Result<RequestProxy<'_>, TransportError> {
        RequestProxy::builder(&self.connection)
            .path(self.path.clone())
            .map_err(bus)?
            .build()
            .await
            .map_err(bus)
    }
}

impl Steering for Calls {
    fn proceed(&self) -> Boxed<'_, Result<(), TransportError>> {
        Box::pin(async move { self.proxy().await?.proceed().await.map_err(bus) })
    }

    fn close(&self) -> Boxed<'_, Result<(), TransportError>> {
        Box::pin(async move { self.proxy().await?.close().await.map_err(bus) })
    }
}

/// Starts a request with `start` and watches it. `Ok(Err(e))` is a method that failed before a
/// request began (the caller reads it as a refusal), as for `awaiting::requested`.
pub(crate) async fn watching(
    connection: &BusConnection,
    start: impl Future<Output = zbus::Result<OwnedObjectPath>>,
    reading: Reading,
) -> Result<Result<Watched, zbus::Error>, TransportError> {
    let dbus = DBusProxy::new(connection).await.map_err(bus)?;
    let name = zbus::names::BusName::try_from(INTENTS_BUS).map_err(bus)?;
    let owner = dbus
        .get_name_owner(name)
        .await
        .map(|owner| owner.to_string())
        .map_err(|_| TransportError::Closed)?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface("org.quire.Intents1.Request")
        .map_err(bus)?
        .build();
    let messages = MessageStream::for_match_rule(rule, connection, Some(256))
        .await
        .map_err(bus)?;
    let leaving = dbus
        .receive_name_owner_changed_with_args(&[(0, INTENTS_BUS)])
        .await
        .map_err(bus)?;
    let path = match start.await {
        Ok(path) => path,
        Err(error) => return Ok(Err(error)),
    };
    Ok(Ok(Watched {
        events: Box::new(Signals {
            messages,
            leaving,
            owner,
            path: path.as_str().to_owned(),
            reading,
            over: false,
        }),
        steering: std::sync::Arc::new(Calls {
            connection: connection.clone(),
            path,
        }),
    }))
}

/// `IntentsReply` of a `Gate.Check` `Response`.
pub(crate) fn gate_reading(answer: (u32, String)) -> Result<IntentsReply, TransportError> {
    crate::bus::response(Ok(answer), IntentsReply::Gate)
}
