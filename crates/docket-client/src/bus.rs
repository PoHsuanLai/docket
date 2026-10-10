//! `DbusTransport::call`: one match from a request to the member of `org.quire.Intents1` that
//! carries it. Bodies are the JSON of the typed value named in `docket-dbus`; a request that
//! `intentd` refuses before it is a call arrives as a bus error (`IntentsError`) and comes back
//! as `IntentsReply::Refused`, a refused call or message as part of its body.
//!
//! Six members answer a Request object (`Run.Perform`, `Run.Undo`, `Run.UndoAll`,
//! `Session.Widen`, `Gate.Grant`, `Gate.Check`): the reply is the `Response` signal at the
//! returned path, whose body is the JSON of the typed answer under code 0, or of a
//! `WireRefusal` under code 2. The signal is subscribed to before the call, so an answer that
//! is quicker than the method's own reply is not lost, and it counts only from intentd's own
//! connection.

use crate::transport::TransportError;
use docket_core::{
    CallRefusal, ContextView, Delivery, IntentsReply, IntentsRequest, Outcome, ReadFault,
    RecallView, SessionOpened, StoredView, TurnId, UndoFault, UndoReport, WidenAnswer, WireRefusal,
};
use docket_dbus::{
    BusConnection, CheckpointProxy, ContextProxy, ControlProxy, Details, GateProxy, INTENTS_BUS,
    IndexProxy, IntentsError, MessageProxy, RegistryProxy, RunProxy, SearchProxy, SessionProxy,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::future::Future;
use zbus::zvariant::OwnedObjectPath;

type Reply = Result<IntentsReply, TransportError>;

fn to_json<T: Serialize>(value: &T) -> Result<String, TransportError> {
    serde_json::to_string(value).map_err(|e| TransportError::Malformed(e.to_string()))
}

fn from_json<T: DeserializeOwned>(text: &str) -> Result<T, TransportError> {
    serde_json::from_str(text).map_err(|e| TransportError::Malformed(e.to_string()))
}

fn bus<E: std::fmt::Display>(error: E) -> TransportError {
    TransportError::Bus(error.to_string())
}

/// What a failed method call is: a request the router refused, or a bus that did not carry it.
fn refusal_of(error: zbus::Error) -> Result<WireRefusal, TransportError> {
    match IntentsError::from(error) {
        IntentsError::NotAllowed(_) => Ok(WireRefusal::NotAllowed),
        IntentsError::NoSuchSession(_) => Ok(WireRefusal::NoSuchSession),
        IntentsError::Malformed(_) => Ok(WireRefusal::Malformed),
        IntentsError::ZBus(error) => Err(fault_of(error)),
        // An error this build does not know yet is the bus's, not a refusal of the request.
        _ => Err(TransportError::Bus(
            "an error the bus library does not name".to_owned(),
        )),
    }
}

/// A bus error that means intentd is not there is `Closed`; anything else is the bus's.
fn fault_of(error: zbus::Error) -> TransportError {
    match &error {
        zbus::Error::MethodError(name, _, _)
            if matches!(
                name.as_str(),
                "org.freedesktop.DBus.Error.ServiceUnknown"
                    | "org.freedesktop.DBus.Error.NameHasNoOwner"
                    | "org.freedesktop.DBus.Error.NoReply"
                    | "org.freedesktop.DBus.Error.Disconnected"
            ) =>
        {
            TransportError::Closed
        }
        zbus::Error::InputOutput(_) => TransportError::Closed,
        _ => bus(error),
    }
}

fn refused(error: zbus::Error) -> Reply {
    refusal_of(error).map(IntentsReply::Refused)
}

/// A method that answers a body.
fn body<T: DeserializeOwned>(
    answer: zbus::Result<String>,
    wrap: impl FnOnce(T) -> IntentsReply,
) -> Reply {
    match answer {
        Ok(text) => Ok(wrap(from_json(&text)?)),
        Err(error) => refused(error),
    }
}

/// A method whose body is `Result<T, CallRefusal>`: the call it stands for may be refused.
fn called<T: DeserializeOwned>(
    answer: zbus::Result<String>,
    wrap: impl FnOnce(T) -> IntentsReply,
) -> Reply {
    body(answer, |end: Result<T, CallRefusal>| match end {
        Ok(value) => wrap(value),
        Err(why) => IntentsReply::Refused(WireRefusal::Call(why)),
    })
}

/// A method that answers nothing.
fn nothing(answer: zbus::Result<()>) -> Reply {
    match answer {
        Ok(()) => Ok(IntentsReply::Done),
        Err(error) => refused(error),
    }
}

/// Starts a request and waits for its `Response`: the code and body, or the error of the
/// method that did not start one.
async fn requested(
    connection: &BusConnection,
    start: impl Future<Output = zbus::Result<OwnedObjectPath>>,
) -> Result<(u32, String), TransportError> {
    match crate::awaiting::requested(connection, INTENTS_BUS, start).await? {
        Ok(response) => Ok(response),
        Err(error) => match refusal_of(error) {
            Ok(refusal) => Ok((2, to_json(&refusal)?)),
            Err(fault) => Err(fault),
        },
    }
}

/// Reads the `Response` of a request: the typed answer under code 0, a refusal under code 2.
pub(crate) fn response<T: DeserializeOwned>(
    answer: Result<(u32, String), TransportError>,
    wrap: impl FnOnce(T) -> IntentsReply,
) -> Reply {
    let (code, reply) = answer?;
    match code {
        0 => Ok(wrap(from_json(&reply)?)),
        2 => Ok(IntentsReply::Refused(from_json(&reply)?)),
        _ => Err(TransportError::Closed),
    }
}

/// The `options` of a request whose caller watches it.
fn watching_options() -> Details {
    let mut options = Details::new();
    if let Ok(on) = zbus::zvariant::OwnedValue::try_from(zbus::zvariant::Value::Bool(true)) {
        options.insert(docket_dbus::OPTION_WATCH.to_owned(), on);
    }
    options
}

/// `options` with the launcher's activation token added, when there is one.
fn with_activation(mut options: Details, token: Option<&docket_core::ActivationToken>) -> Details {
    let value = token.and_then(|t| {
        zbus::zvariant::OwnedValue::try_from(zbus::zvariant::Value::from(t.as_str())).ok()
    });
    if let Some(value) = value {
        options.insert(docket_dbus::OPTION_ACTIVATION.to_owned(), value);
    }
    options
}

/// `Gate.Check` and `Run.Perform` for a caller that watches them: the option tells intentd to say
/// how far the request is, and for a gate check to wait for `Proceed` before it draws the sheet.
/// Any other request is answered in one piece.
pub(crate) async fn watch(
    connection: &BusConnection,
    request: IntentsRequest,
) -> Result<crate::watch::Watched, TransportError> {
    let options = watching_options();
    match request {
        IntentsRequest::GateCheck(ask) => {
            let proxy = GateProxy::new(connection).await.map_err(bus)?;
            let ask = to_json(&ask)?;
            let start = proxy.check(&ask, &options);
            let started =
                crate::watch_bus::watching(connection, start, crate::watch_bus::gate_reading)
                    .await?;
            match started {
                Ok(watched) => Ok(watched),
                Err(error) => Ok(crate::watch::Watched::answered(refused(error)?)),
            }
        }
        IntentsRequest::Perform {
            call,
            session,
            parent_window,
            activation,
        } => {
            let options = with_activation(options, activation.as_ref());
            let run = RunProxy::new(connection).await.map_err(bus)?;
            let (call, session, window) = (
                to_json(&call)?,
                to_json(&session)?,
                to_json(&parent_window)?,
            );
            let start = run.perform(&call, &session, &window, &options);
            let started =
                crate::watch_bus::watching(connection, start, crate::watch_bus::perform_reading)
                    .await?;
            match started {
                Ok(watched) => Ok(watched),
                Err(error) => Ok(crate::watch::Watched::answered(refused(error)?)),
            }
        }
        other => Ok(crate::watch::Watched::answered(
            call(connection, other).await?,
        )),
    }
}

/// Sends `request` over `connection` and reads its one reply.
pub(crate) async fn call(connection: &BusConnection, request: IntentsRequest) -> Reply {
    use IntentsRequest as Q;
    let c = connection;
    let options = Details::new();
    match request {
        Q::Manifests => {
            let rows = RegistryProxy::new(c).await.map_err(bus)?.manifests().await;
            match rows {
                Ok(rows) => {
                    let manifests = rows
                        .iter()
                        .map(|(_, text)| from_json(text))
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(IntentsReply::Manifests(manifests))
                }
                Err(error) => refused(error),
            }
        }
        Q::IndexPush(batch) => {
            let index = IndexProxy::new(c).await.map_err(bus)?;
            nothing(index.push(&to_json(&batch)?).await)
        }
        Q::IndexReset { epoch } => {
            let index = IndexProxy::new(c).await.map_err(bus)?;
            nothing(index.reset(epoch).await)
        }
        Q::Search(ask) => {
            let search = SearchProxy::new(c).await.map_err(bus)?;
            let answer = search
                .query(&ask.text, &to_json(&ask.scope)?, ask.generation.0)
                .await;
            body(answer, IntentsReply::Hits)
        }
        Q::SearchCancel(generation) => {
            let search = SearchProxy::new(c).await.map_err(bus)?;
            nothing(search.cancel(generation.0).await)
        }
        Q::Perform {
            call,
            session,
            parent_window,
            activation,
        } => {
            let options = with_activation(options, activation.as_ref());
            let run = RunProxy::new(c).await.map_err(bus)?;
            let (call, session, window) = (
                to_json(&call)?,
                to_json(&session)?,
                to_json(&parent_window)?,
            );
            let start = run.perform(&call, &session, &window, &options);
            response(
                requested(c, start).await,
                |end: Result<Outcome, CallRefusal>| IntentsReply::Performed(Box::new(end)),
            )
        }
        Q::DryRun { call, session } => {
            let run = RunProxy::new(c).await.map_err(bus)?;
            let answer = run.dry_run(&to_json(&call)?, &to_json(&session)?).await;
            called(answer, IntentsReply::Preview)
        }
        Q::Preview(entity) => {
            let run = RunProxy::new(c).await.map_err(bus)?;
            called(run.preview(&to_json(&entity)?).await, IntentsReply::Preview)
        }
        Q::Suggest(ask) => {
            let run = RunProxy::new(c).await.map_err(bus)?;
            called(
                run.suggest(&to_json(&ask)?).await,
                IntentsReply::Suggestions,
            )
        }
        Q::Undo(entry) => {
            let run = RunProxy::new(c).await.map_err(bus)?;
            response(
                requested(c, run.undo(entry.0)).await,
                |end: Result<(), UndoFault>| IntentsReply::Undone(end),
            )
        }
        Q::UndoAll(scope) => {
            let run = RunProxy::new(c).await.map_err(bus)?;
            let scope = to_json(&scope)?;
            let start = run.undo_all(&scope);
            response(requested(c, start).await, |r: UndoReport| {
                IntentsReply::UndoneAll(r)
            })
        }
        Q::Context { session, app } => {
            let context = ContextProxy::new(c).await.map_err(bus)?;
            let answer = context.current(session.as_str(), app.as_str()).await;
            called(answer, |view: ContextView| {
                IntentsReply::Context(Box::new(view))
            })
        }
        Q::SessionOpen(open) => {
            let session = SessionProxy::new(c).await.map_err(bus)?;
            let answer = session.open(&to_json(&open)?).await;
            body(answer, |s: SessionOpened| IntentsReply::SessionOpened(s))
        }
        Q::SessionTurn { session, turn } => {
            let proxy = SessionProxy::new(c).await.map_err(bus)?;
            match proxy
                .turn(session.as_str(), &to_json(&turn)?, &options)
                .await
            {
                Ok(id) => Ok(IntentsReply::TurnRecorded(TurnId(id))),
                Err(error) => refused(error),
            }
        }
        Q::SessionClose { session } => {
            let proxy = SessionProxy::new(c).await.map_err(bus)?;
            nothing(proxy.close(session.as_str()).await)
        }
        Q::SessionTurnEnded { session, turn, how } => {
            let proxy = SessionProxy::new(c).await.map_err(bus)?;
            nothing(
                proxy
                    .turn_ended(session.as_str(), turn.0, &to_json(&how)?)
                    .await,
            )
        }
        Q::SessionResolve { session, handle } => {
            let proxy = SessionProxy::new(c).await.map_err(bus)?;
            body(
                proxy.resolve(session.as_str(), handle.0).await,
                IntentsReply::Resolved,
            )
        }
        Q::SessionDisplay { session, handle } => {
            let proxy = SessionProxy::new(c).await.map_err(bus)?;
            match proxy.display(session.as_str(), handle.0).await {
                Ok(text) => Ok(IntentsReply::Text(text)),
                Err(error) => refused(error),
            }
        }
        Q::SessionDisplayLabelled { session, handle } => {
            let proxy = SessionProxy::new(c).await.map_err(bus)?;
            body(
                proxy.display_labelled(session.as_str(), handle.0).await,
                IntentsReply::Displayed,
            )
        }
        Q::SessionRead { session, ask } => {
            let proxy = SessionProxy::new(c).await.map_err(bus)?;
            let answer = proxy
                .read(session.as_str(), &to_json(&ask.ask)?, &options)
                .await;
            body(answer, |end: Result<_, ReadFault>| match end {
                Ok(value) => IntentsReply::Read(value),
                Err(fault) => IntentsReply::Refused(WireRefusal::Read(fault)),
            })
        }
        Q::SessionTaskPolicy { session } => {
            let proxy = SessionProxy::new(c).await.map_err(bus)?;
            body(
                proxy.task_policy(session.as_str()).await,
                |policy: Option<docket_core::TaskPolicy>| {
                    IntentsReply::TaskPolicy(policy.map(Box::new))
                },
            )
        }
        Q::SessionWiden { session, widen } => {
            let proxy = SessionProxy::new(c).await.map_err(bus)?;
            let change = to_json(&widen.change)?;
            let start = proxy.widen(session.as_str(), widen.turn.0, &change);
            response(requested(c, start).await, |a: WidenAnswer| {
                IntentsReply::Widened(a)
            })
        }
        Q::SessionNote { session, note } => {
            let proxy = SessionProxy::new(c).await.map_err(bus)?;
            nothing(proxy.note(session.as_str(), &to_json(&note)?).await)
        }
        Q::SessionNarrow { session, turn } => {
            let proxy = SessionProxy::new(c).await.map_err(bus)?;
            nothing(proxy.narrow(session.as_str(), &to_json(&turn)?).await)
        }
        Q::SessionHandles { session } => {
            let proxy = SessionProxy::new(c).await.map_err(bus)?;
            body(
                proxy.handles(session.as_str()).await,
                |cards: Vec<docket_core::HandleCard>| IntentsReply::Handles(cards),
            )
        }
        Q::SessionRecall { session, ask } => {
            let proxy = SessionProxy::new(c).await.map_err(bus)?;
            let answer = proxy
                .recall(session.as_str(), &to_json(&ask)?, &options)
                .await;
            body(answer, |view: RecallView| IntentsReply::Recalled(view))
        }
        Q::SessionStored { ask } => {
            let proxy = SessionProxy::new(c).await.map_err(bus)?;
            body(proxy.stored(&to_json(&ask)?).await, |view: StoredView| {
                IntentsReply::Stored(view)
            })
        }
        Q::CheckpointList { session } => {
            let proxy = CheckpointProxy::new(c).await.map_err(bus)?;
            body(
                proxy.list(session.as_str()).await,
                |list: docket_core::CheckpointList| IntentsReply::Checkpoints(Box::new(list)),
            )
        }
        Q::CheckpointPlan { session, id } => {
            let proxy = CheckpointProxy::new(c).await.map_err(bus)?;
            body(
                proxy.plan(session.as_str(), id.0).await,
                IntentsReply::CheckpointPlan,
            )
        }
        Q::CheckpointWatch {
            workspace,
            label,
            rewind,
        } => {
            let proxy = CheckpointProxy::new(c).await.map_err(bus)?;
            body(
                proxy
                    .watch(workspace.as_str(), &label, &to_json(&rewind)?)
                    .await,
                IntentsReply::CheckpointWatching,
            )
        }
        Q::CheckpointMark { session } => {
            let proxy = CheckpointProxy::new(c).await.map_err(bus)?;
            match proxy.mark(session.as_str()).await {
                Ok(id) => Ok(IntentsReply::TurnRecorded(TurnId(id))),
                Err(error) => refused(error),
            }
        }
        Q::MessageSend { session, draft } => {
            let proxy = MessageProxy::new(c).await.map_err(bus)?;
            let answer = proxy
                .send(session.as_str(), &to_json(&draft)?, &options)
                .await;
            body(
                answer,
                |end: Result<Delivery, docket_core::SendRefusal>| match end {
                    Ok(delivery) => IntentsReply::Delivered(delivery),
                    Err(why) => IntentsReply::Refused(WireRefusal::Send(why)),
                },
            )
        }
        Q::MessageInbox(ask) => {
            let proxy = MessageProxy::new(c).await.map_err(bus)?;
            body(proxy.inbox(&to_json(&ask)?).await, IntentsReply::Inbox)
        }
        Q::GateGrant(ask) => {
            let proxy = GateProxy::new(c).await.map_err(bus)?;
            let start = proxy.grant(ask.app.as_str(), ask.space.as_str());
            response(requested(c, start).await, IntentsReply::Granted)
        }
        Q::GateCheck(ask) => {
            let proxy = GateProxy::new(c).await.map_err(bus)?;
            let ask = to_json(&ask)?;
            let start = proxy.check(&ask, &options);
            response(requested(c, start).await, IntentsReply::Gate)
        }
        Q::ControlHalt { scope, cause } => {
            let proxy = ControlProxy::new(c).await.map_err(bus)?;
            nothing(proxy.halt(&to_json(&scope)?, &to_json(&cause)?).await)
        }
        Q::ControlResume { scope } => {
            let proxy = ControlProxy::new(c).await.map_err(bus)?;
            nothing(proxy.resume(&to_json(&scope)?).await)
        }
        Q::ControlState => {
            let proxy = ControlProxy::new(c).await.map_err(bus)?;
            body(proxy.state().await, IntentsReply::State)
        }
        Q::ControlJournal(filter) => {
            let proxy = ControlProxy::new(c).await.map_err(bus)?;
            body(
                proxy.journal(&to_json(&filter)?).await,
                IntentsReply::Journal,
            )
        }
        Q::ControlTerminalGrants => {
            let proxy = ControlProxy::new(c).await.map_err(bus)?;
            body(proxy.terminal_grants().await, IntentsReply::TerminalGrants)
        }
        Q::ControlTerminalRevoke(action) => {
            let proxy = ControlProxy::new(c).await.map_err(bus)?;
            nothing(proxy.revoke_terminal_grant(&to_json(&action)?).await)
        }
        Q::ControlStandingGrants => {
            let proxy = ControlProxy::new(c).await.map_err(bus)?;
            body(proxy.standing_grants().await, IntentsReply::StandingGrants)
        }
        Q::ControlStandingRevoke(id) => {
            let proxy = ControlProxy::new(c).await.map_err(bus)?;
            nothing(proxy.revoke_standing_grant(&to_json(&id)?).await)
        }
        // A request with no bus member is an error, never a silent success.
        _ => Err(TransportError::Malformed(
            "a request the bus has no member for".to_owned(),
        )),
    }
}
