//! Request objects: a call that may wait for the person is answered with the path of an object
//! at `/org/quire/Intents1/request/<n>`, and the answer is its `Response` signal, sent to the
//! caller alone (an answer carries what the call returned). The path is returned before the
//! answer is awaited; a caller subscribes before it calls, so a quick answer is not lost.

use super::{Gateway, render};
use docket_core::{CallId, IntentsReply, IntentsRequest, WireRefusal};
use docket_dbus::{IntentsError, request_path};
use docket_router::{Flag, Watch};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::task::AbortHandle;
use zbus::message::Header;
use zbus::zvariant::OwnedObjectPath;

/// The code of an answer: done, or refused before it was a call.
const DONE: u32 = 0;
const REFUSED: u32 = 2;

/// Whether the caller said it watches the request (`Gate.Check`'s `watch` option): it then
/// hears `Progress`, and the router waits for its `Proceed` before it draws a sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Watching {
    /// It listens and steers.
    Yes,
    /// It waits for the `Response` alone.
    No,
}

/// What a watching caller says back.
#[derive(Clone)]
struct Steer {
    proceed: Arc<Flag>,
    closed: Arc<Flag>,
}

/// One request in flight. Only the connection that made it may close it.
pub(crate) struct RequestObject {
    owner: String,
    work: AbortHandle,
    steer: Option<Steer>,
}

impl RequestObject {
    fn owned_by(&self, header: &Header<'_>) -> Result<(), IntentsError> {
        match header.sender().is_some_and(|s| s.as_str() == self.owner) {
            true => Ok(()),
            false => Err(IntentsError::NotAllowed("not your request".into())),
        }
    }
}

#[zbus::interface(name = "org.quire.Intents1.Request")]
impl RequestObject {
    async fn close(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(object_server)] server: &zbus::ObjectServer,
    ) -> Result<(), IntentsError> {
        self.owned_by(&header)?;
        let _ = connection;
        match &self.steer {
            // A watched request ends itself: the router takes its sheet back, and no `Response`
            // follows.
            Some(steer) => steer.closed.raise(),
            None => {
                self.work.abort();
                let path = header.path().map(|p| p.to_owned());
                if let Some(path) = path {
                    let _ = server.remove::<RequestObject, _>(path).await;
                }
            }
        }
        Ok(())
    }

    async fn proceed(&self, #[zbus(header)] header: Header<'_>) -> Result<(), IntentsError> {
        self.owned_by(&header)?;
        match &self.steer {
            Some(steer) => {
                steer.proceed.raise();
                Ok(())
            }
            None => Err(IntentsError::Malformed(
                "this request is not watched: nothing waits for Proceed".into(),
            )),
        }
    }

    #[zbus(signal)]
    async fn progress(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        progress: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn response(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        code: u32,
        reply: &str,
    ) -> zbus::Result<()>;
}

/// The code and body a reply is sent as: the typed answer under 0, a refusal under 2.
fn response_of(reply: IntentsReply) -> Result<(u32, String), IntentsError> {
    match reply {
        IntentsReply::Refused(why) => Ok((REFUSED, render(&why)?)),
        IntentsReply::Performed(end) => Ok((DONE, render(&*end)?)),
        IntentsReply::Undone(end) => Ok((DONE, render(&end)?)),
        IntentsReply::UndoneAll(report) => Ok((DONE, render(&report)?)),
        IntentsReply::Widened(answer) => Ok((DONE, render(&answer)?)),
        IntentsReply::Granted(answer) => Ok((DONE, render(&answer)?)),
        IntentsReply::Gate(answer) => Ok((DONE, render(&answer)?)),
        _ => Ok((REFUSED, render(&WireRefusal::Malformed)?)),
    }
}

/// The router's end of a watched request: progress is a `Progress` signal to the caller alone,
/// `Proceed` and `Close` are the flags the Request object raises.
fn watch_of(connection: zbus::Connection, to: String, at: String, steer: &Steer) -> Watch {
    let (proceed, closed) = (steer.proceed.clone(), steer.closed.clone());
    Watch::new(
        move |progress| {
            let (connection, to, at) = (connection.clone(), to.clone(), at.clone());
            Box::pin(async move {
                if let Ok(text) = render(&progress) {
                    let _ = connection
                        .emit_signal(
                            Some(to.as_str()),
                            at.as_str(),
                            "org.quire.Intents1.Request",
                            "Progress",
                            &(text,),
                        )
                        .await;
                }
            })
        },
        move || Box::pin(proceed.up()),
        move || Box::pin(closed.up()),
    )
}

impl Gateway {
    /// Starts `request` as the caller of `header` and answers the path of its Request object.
    pub(crate) async fn start(
        &self,
        header: &Header<'_>,
        connection: &zbus::Connection,
        request: IntentsRequest,
    ) -> Result<OwnedObjectPath, IntentsError> {
        self.start_as(header, connection, request, Watching::No)
            .await
    }

    /// `start`, for a caller that may be watching.
    pub(crate) async fn start_as(
        &self,
        header: &Header<'_>,
        connection: &zbus::Connection,
        request: IntentsRequest,
        watching: Watching,
    ) -> Result<OwnedObjectPath, IntentsError> {
        let caller = self.caller(header).await?;
        let sender = header
            .sender()
            .map(|s| s.to_owned())
            .ok_or_else(|| IntentsError::NotAllowed("no sender".into()))?;
        let number = self.requests.fetch_add(1, Ordering::Relaxed) + 1;
        let path = request_path(CallId(number));
        let handler = self.handler.clone();
        let emitting = connection.clone();
        let at = path.clone();
        let to = sender.clone();
        let (go, wait) = tokio::sync::oneshot::channel::<()>();
        let steer = match watching {
            Watching::Yes => Some(Steer {
                proceed: Flag::new(),
                closed: Flag::new(),
            }),
            Watching::No => None,
        };
        let watch = steer
            .clone()
            .map(|s| watch_of(connection.clone(), to.to_string(), at.clone(), &s))
            .unwrap_or_else(Watch::none);
        let withdrawn = steer.clone();
        let work = tokio::spawn(async move {
            // The object is exported before the router is asked, so its removal below always
            // finds it.
            if wait.await.is_err() {
                return;
            }
            let reply = handler(caller, request, watch).await;
            if withdrawn.is_some_and(|s| s.closed.is_up()) {
                let _ = emitting
                    .object_server()
                    .remove::<RequestObject, _>(at.as_str())
                    .await;
                return;
            }
            let (code, body) = response_of(reply).unwrap_or_else(|_| (REFUSED, String::new()));
            let _ = emitting
                .emit_signal(
                    Some(to.as_str()),
                    at.as_str(),
                    "org.quire.Intents1.Request",
                    "Response",
                    &(code, body),
                )
                .await;
            let _ = emitting
                .object_server()
                .remove::<RequestObject, _>(at.as_str())
                .await;
        });
        let object = RequestObject {
            owner: sender.to_string(),
            work: work.abort_handle(),
            steer,
        };
        connection
            .object_server()
            .at(path.as_str(), object)
            .await
            .map_err(IntentsError::ZBus)?;
        let _ = go.send(());
        OwnedObjectPath::try_from(path).map_err(|e| IntentsError::Malformed(e.to_string()))
    }
}
