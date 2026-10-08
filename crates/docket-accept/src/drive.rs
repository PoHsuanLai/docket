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
use porter_core::AppName;
use prov::{AgentRef, SpaceId, UnixSeconds};
use std::time::Duration;
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
    patience: Duration,
    summoned: Option<AppName>,
}

impl Launcher {
    /// The launcher of `world`.
    pub async fn of(world: &World) -> Launcher {
        let connection = world.sill.clone();
        Launcher {
            intents: Intents::over(DbusTransport::new(connection.clone())),
            companion: CompanionProxy::new(&connection).await.expect("proxy"),
            connection,
            patience: GIVE_UP,
            summoned: None,
        }
    }

    /// The same launcher, asking from the window of `app`: the companion reads where the person
    /// is there (the thread they have open) when it resolves "this".
    pub fn summoned_from(self, app: AppName) -> Launcher {
        Launcher {
            summoned: Some(app),
            ..self
        }
    }

    /// The same launcher, waiting this long for each change of an answer instead of
    /// [`GIVE_UP`]: a real model on a cold engine is slower than a cassette.
    pub fn patient(self, patience: Duration) -> Launcher {
        Launcher { patience, ..self }
    }

    /// Opens a front conversation in the Space `work`.
    pub async fn open(&self) -> SessionOpened {
        let open = SessionOpen {
            space: SpaceId::parse("work").expect("space"),
            agent: AgentRef::Companion,
            parent: None,
            cwd: None,
            started_from: None,
            external: None,
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
            app: self.summoned.clone(),
        };
        let path: OwnedObjectPath = self
            .companion
            .ask(
                &serde_json::to_string(&ask).expect("json"),
                &Default::default(),
            )
            .await
            .expect("Companion1.Ask");
        Answer::watch(&self.connection, path, self.patience).await
    }
}

/// One answer object, watched.
pub struct Answer {
    patience: Duration,
    proxy: CompanionAnswerProxy<'static>,
    updates: std::pin::Pin<Box<dyn futures_util::Stream<Item = String> + Send>>,
}

impl std::fmt::Debug for Answer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Answer")
    }
}

impl Answer {
    async fn watch(
        connection: &zbus::Connection,
        path: OwnedObjectPath,
        patience: Duration,
    ) -> Answer {
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
            patience,
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
        match self.try_history_until(stop).await {
            Ok(seen) => seen,
            Err(seen) => panic!("the answer never settled; saw {seen:#?}"),
        }
    }

    /// [`Answer::history_until`] that hands back what it saw, as the error, when nothing
    /// changed for the answer's patience (or the signal stream ended).
    pub async fn try_history_until(
        &mut self,
        stop: impl Fn(&AnswerWire) -> bool,
    ) -> Result<Vec<AnswerWire>, Vec<AnswerWire>> {
        let mut seen: Vec<AnswerWire> = Vec::new();
        let take = |view: AnswerWire, seen: &mut Vec<AnswerWire>| {
            let last = stop(&view);
            if seen.last() != Some(&view) {
                seen.push(view);
            }
            last
        };
        if take(self.view().await, &mut seen) {
            return Ok(seen);
        }
        loop {
            let Ok(Some(next)) = tokio::time::timeout(self.patience, self.updates.next()).await
            else {
                return Err(seen);
            };
            let Ok(view) = serde_json::from_str::<AnswerWire>(&next) else {
                return Err(seen);
            };
            if take(view, &mut seen) {
                return Ok(seen);
            }
        }
    }
}

/// What memoryd holds of the Spaces `work` and `desktop` as the shell reads it (the shell's identity is this
/// test process, placed in the fake proc root as sill): the recent entries, polled until
/// every kind in `kinds` is among them (the audit kinds of `intentd`'s `record.rs`). intentd drains its audit queue into memoryd on a timer, so
/// there is no event to wait on; the bound only turns a missing record into a failure.
pub async fn recorded(world: &World, kinds: &[&str]) -> Vec<almanac_core::RecentEntry> {
    use almanac_client::{DbusTransport, Memory};
    use almanac_core::{BodyMode, RecentQuery, TrustFilter};
    let memory = Memory::over(DbusTransport::new(world.sill.clone()));
    let spaces = ["work", "desktop"].map(|s| SpaceId::parse(s).expect("space"));
    let seen = std::cell::RefCell::new(Vec::<String>::new());
    let poll = async {
        loop {
            let mut entries = Vec::new();
            for space in &spaces {
                let query = RecentQuery {
                    since: UnixSeconds(0),
                    kinds: vec![],
                    trust: TrustFilter::Any,
                    limit: porter_core::Count(100),
                    bodies: BodyMode::Json,
                };
                entries.extend(
                    memory
                        .recent(space.clone(), query)
                        .await
                        .unwrap_or_default(),
                );
            }
            *seen.borrow_mut() = entries
                .iter()
                .map(|e| format!("{:?}", e.summary.kind))
                .collect();
            if kinds.iter().all(|k| {
                entries
                    .iter()
                    .any(|e| format!("{:?}", e.summary.kind) == format!("KindTag(\"{k}\")"))
            }) {
                return entries;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    };
    tokio::time::timeout(GIVE_UP, poll)
        .await
        .unwrap_or_else(|_| {
            panic!(
                "memoryd never held {kinds:?}; it holds {:?}\n{}",
                seen.borrow(),
                world.logs()
            )
        })
}
