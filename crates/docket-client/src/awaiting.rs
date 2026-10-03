//! Waiting for the answer of a Request object: the portal shape `org.quire.Intents1` and
//! `org.quire.Confirm1` share. The method returns the path of an object at once; the answer is
//! its `Response(code, body)` signal, sent to the caller alone.

use crate::transport::TransportError;
use docket_dbus::BusConnection;
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

/// The unique name that owns `service`, so a `Response` signal counts only from it.
async fn owner_of(connection: &BusConnection, service: &str) -> Result<String, TransportError> {
    let dbus = DBusProxy::new(connection).await.map_err(bus)?;
    let name = zbus::names::BusName::try_from(service).map_err(bus)?;
    dbus.get_name_owner(name)
        .await
        .map(|owner| owner.to_string())
        .map_err(|_| TransportError::Closed)
}

/// Starts a request with `start` and waits for its `Response`: `Ok(Ok((code, body)))`, or
/// `Ok(Err(e))` when the method itself failed (a request refused before it started). The
/// signal is subscribed to before `start` runs, so an answer quicker than the method's own
/// reply is not lost; it counts only from the owner of `service`, and the wait ends `Closed`
/// when that owner leaves the bus.
pub async fn requested(
    connection: &BusConnection,
    service: &str,
    start: impl Future<Output = zbus::Result<OwnedObjectPath>>,
) -> Result<Result<(u32, String), zbus::Error>, TransportError> {
    let owner = owner_of(connection, service).await?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface("org.quire.Intents1.Request")
        .and_then(|b| b.member("Response"))
        .map_err(bus)?
        .build();
    let mut responses = MessageStream::for_match_rule(rule, connection, Some(256))
        .await
        .map_err(bus)?;
    let dbus = DBusProxy::new(connection).await.map_err(bus)?;
    let mut leaving = dbus
        .receive_name_owner_changed_with_args(&[(0, service)])
        .await
        .map_err(bus)?;
    let path = match start.await {
        Ok(path) => path,
        Err(error) => return Ok(Err(error)),
    };
    loop {
        let next = select(Box::pin(responses.next()), Box::pin(leaving.next())).await;
        match next {
            Either::Left((Some(Ok(message)), _)) => {
                let header = message.header();
                let from_owner = header.sender().map(|s| s.as_str()) == Some(owner.as_str());
                if from_owner && header.path().map(|p| p.as_str()) == Some(path.as_str()) {
                    return message.body().deserialize().map(Ok).map_err(bus);
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
}
