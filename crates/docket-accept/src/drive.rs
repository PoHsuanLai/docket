//! The launcher's side of the run, over the bus: what sill and the launcher do for the person.
//! Open a conversation, record the turn with intentd, hand it to companiond, and watch the
//! answer object until it is done. Every step is a call or a signal on the private bus.

use crate::world::{GIVE_UP, World};
use companion_wire::{AnswerWire, AskWire};
use docket_client::{DbusTransport, Intents};
use docket_core::{
    ContextKeep, Keep, Origin, SessionOpen, SessionOpened, TurnIn, TurnSource, TurnVia, UserTurn,
    WindowKey,
};
use docket_dbus::{CompanionAnswerProxy, CompanionProxy};
use futures_util::StreamExt;
use prov::{AgentRef, SpaceId, UnixSeconds};
use zbus::proxy::CacheProperties;
use zbus::zvariant::OwnedObjectPath;

/// Nothing of the screen is kept with a prompt.
pub fn keep_nothing() -> ContextKeep {
    ContextKeep {
        query: Keep::Dropped,
        results: Keep::Dropped,
        selection: Keep::Dropped,
        window: Keep::Dropped,
    }
}

/// sill's calls into intentd and companiond: the connection owns `org.quire.Shell`, which
/// `intentd.toml` gives the launcher, confirm and control roles and `companiond.toml` the
/// right to speak for the person.
#[derive(Debug)]
pub struct Launcher {
    /// intentd, as the launcher.
    pub intents: Intents<DbusTransport>,
    connection: zbus::Connection,
    companion: CompanionProxy<'static>,
}

impl Launcher {
    /// The launcher of `world`.
    pub async fn of(world: &World) -> Launcher {
        let connection = world.sill.clone();
        Launcher {
            intents: Intents::over(DbusTransport::new(connection.clone())),
            companion: CompanionProxy::new(&connection).await.expect("proxy"),
            connection,
        }
    }

    /// Opens a front conversation in the Space `work`.
    pub async fn open(&self) -> SessionOpened {
        let open = SessionOpen {
            space: SpaceId::parse("work").expect("space"),
            agent: AgentRef::Companion,
            parent: None,
        };
        let text = self
            .companion
            .open(&serde_json::to_string(&open).expect("json"))
            .await
            .expect("Companion1.Open");
        serde_json::from_str(&text).expect("SessionOpened")
    }

    /// The person says `text`: intentd records the turn, companiond is asked. Returns the answer
    /// being watched, subscribed before the loop can say anything.
    pub async fn say(&self, opened: &SessionOpened, text: &str) -> Answer {
        let id = self
            .intents
            .session_turn(
                opened.session.clone(),
                TurnIn {
                    text: text.into(),
                    origin: Origin::Launcher,
                    keep: keep_nothing(),
                    via: TurnVia::Typed,
                },
            )
            .await
            .expect("Session.Turn");
        let ask = AskWire {
            session: opened.session.clone(),
            turn: UserTurn {
                id,
                text: text.into(),
                at: UnixSeconds(1),
                from: TurnSource::Launcher,
                via: TurnVia::Typed,
            },
            keep: keep_nothing(),
            parent_window: WindowKey::parse("w1").expect("window"),
            app: None,
        };
        let path: OwnedObjectPath = self
            .companion
            .ask(
                &serde_json::to_string(&ask).expect("json"),
                &Default::default(),
            )
            .await
            .expect("Companion1.Ask");
        Answer::watch(&self.connection, path).await
    }
}

/// One answer object, watched.
pub struct Answer {
    proxy: CompanionAnswerProxy<'static>,
    updates: std::pin::Pin<Box<dyn futures_util::Stream<Item = String> + Send>>,
}

impl std::fmt::Debug for Answer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Answer")
    }
}

impl Answer {
    async fn watch(connection: &zbus::Connection, path: OwnedObjectPath) -> Answer {
        let proxy = CompanionAnswerProxy::builder(connection)
            .path(path)
            .expect("answer path")
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .expect("answer proxy");
        let updates = proxy
            .receive_updated()
            .await
            .expect("Updated signal")
            .filter_map(|signal| async move { signal.args().ok().map(|a| a.view().to_string()) });
        Answer {
            proxy,
            updates: Box::pin(updates),
        }
    }

    /// The answer as it stands now.
    pub async fn view(&self) -> AnswerWire {
        let text = self.proxy.view().await.expect("Answer.View");
        serde_json::from_str(&text).expect("AnswerWire")
    }

    /// Every distinct view the answer takes, in order, until `stop` says one is the last. The
    /// initial view and every `Updated` payload count, so a phase that passes quickly (a sheet
    /// answered at once) is still in the list.
    pub async fn history_until(&mut self, stop: impl Fn(&AnswerWire) -> bool) -> Vec<AnswerWire> {
        let mut seen: Vec<AnswerWire> = Vec::new();
        let take = |view: AnswerWire, seen: &mut Vec<AnswerWire>| {
            let last = stop(&view);
            if seen.last() != Some(&view) {
                seen.push(view);
            }
            last
        };
        if take(self.view().await, &mut seen) {
            return seen;
        }
        loop {
            let next = tokio::time::timeout(GIVE_UP, self.updates.next())
                .await
                .unwrap_or_else(|_| panic!("the answer never settled; saw {seen:#?}"))
                .expect("the signal stream ended");
            let view: AnswerWire = serde_json::from_str(&next).expect("AnswerWire");
            if take(view, &mut seen) {
                return seen;
            }
        }
    }
}

/// What memoryd holds of the Space `work` as the shell reads it (the shell's identity is this
/// test process's executable, named in `memory-callers.toml`): the recent entries, polled until
/// there are `at_least` of them. intentd drains its audit queue into memoryd on a timer, so
/// there is no event to wait on; the bound only turns a missing record into a failure.
pub async fn recorded(world: &World, at_least: usize) -> Vec<almanac_core::RecentEntry> {
    use almanac_client::{DbusTransport, Memory};
    use almanac_core::{BodyMode, RecentQuery, TrustFilter};
    let memory = Memory::over(DbusTransport::new(world.sill.clone()));
    let space = SpaceId::parse("work").expect("space");
    let poll = async {
        loop {
            let entries = memory
                .recent(
                    space.clone(),
                    RecentQuery {
                        since: UnixSeconds(0),
                        kinds: vec![],
                        trust: TrustFilter::Any,
                        limit: porter_core::Count(100),
                        bodies: BodyMode::Json,
                    },
                )
                .await
                .unwrap_or_default();
            if entries.len() >= at_least {
                return entries;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    };
    tokio::time::timeout(GIVE_UP, poll)
        .await
        .unwrap_or_else(|_| panic!("memoryd never held {at_least} records\n{}", world.logs()))
}
