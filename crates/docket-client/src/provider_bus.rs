//! `org.quire.IntentProvider1` served for one provider: each member decodes the JSON of its
//! typed argument, calls the provider and answers the JSON of its typed result. The member
//! list, its signatures and its signals are those of `docket-dbus`'s `IntentProviderSkeleton`
//! (a test holds the introspection to `dbus/org.quire.IntentProvider1.xml`).
//!
//! The gate lives in intentd: a provider answers nothing but intentd, so a process that calls
//! an app's bus name directly gets no way around it. `Summon` is the one member that also comes
//! from the shell.

use crate::provider::{ContextSource, IntentProvider, SummonTarget};
use crate::transport::TransportError;
use docket_core::{SummonSerial, ValidManifest};
use docket_dbus::{BusConnection, Details, INTENTS_BUS, PROVIDER_PATH};
use futures_util::StreamExt;
use serde::Serialize;
use serde::de::DeserializeOwned;
use zbus::fdo;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;

fn parse<T: DeserializeOwned>(text: &str) -> fdo::Result<T> {
    serde_json::from_str(text).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))
}

fn render<T: Serialize>(value: &T) -> fdo::Result<String> {
    serde_json::to_string(value).map_err(|e| fdo::Error::Failed(e.to_string()))
}

/// One provider with the two seams ds answers for it.
struct ProviderObject<P, C, S> {
    provider: P,
    context: C,
    summon: S,
}

/// Whether the caller is intentd: the owner of its well-known name.
async fn from_intentd(connection: &zbus::Connection, header: &Header<'_>) -> fdo::Result<()> {
    let dbus = fdo::DBusProxy::new(connection).await?;
    let name = zbus::names::BusName::try_from(INTENTS_BUS)
        .map_err(|e| fdo::Error::Failed(e.to_string()))?;
    let owner = dbus.get_name_owner(name).await;
    match (owner, header.sender()) {
        (Ok(owner), Some(sender)) if owner.as_str() == sender.as_str() => Ok(()),
        _ => Err(fdo::Error::AccessDenied(
            "only intentd calls a provider".into(),
        )),
    }
}

#[zbus::interface(name = "org.quire.IntentProvider1")]
impl<P, C, S> ProviderObject<P, C, S>
where
    P: IntentProvider + 'static,
    C: ContextSource + 'static,
    S: SummonTarget + 'static,
{
    async fn perform(
        &self,
        invocation: String,
        options: Details,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<String> {
        let _ = options;
        from_intentd(connection, &header).await?;
        let delivered: docket_core::ActivatedInvocation = parse(&invocation)?;
        render(
            &self
                .provider
                .perform_classified(
                    delivered.invocation,
                    delivered.activation,
                    delivered.classified,
                )
                .await,
        )
    }

    async fn classify(
        &self,
        invocation: String,
        options: Details,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<String> {
        let _ = options;
        from_intentd(connection, &header).await?;
        render(&self.provider.classify(parse(&invocation)?).await)
    }

    async fn dry_run(
        &self,
        invocation: String,
        options: Details,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<String> {
        let _ = options;
        from_intentd(connection, &header).await?;
        render(&self.provider.dry_run(parse(&invocation)?).await)
    }

    async fn undo(
        &self,
        token: String,
        actor: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<String> {
        from_intentd(connection, &header).await?;
        render(&self.provider.undo(parse(&token)?, parse(&actor)?).await)
    }

    async fn context(
        &self,
        scope: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<String> {
        from_intentd(connection, &header).await?;
        render(&self.context.snapshot(parse(&scope)?))
    }

    async fn summon(&self, serial: u64, origin: String) -> fdo::Result<String> {
        render(&self.summon.summon(SummonSerial(serial), parse(&origin)?))
    }

    async fn search(
        &self,
        text: String,
        generation: u64,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<String> {
        let _ = generation;
        from_intentd(connection, &header).await?;
        render(&self.provider.search(&text).await)
    }

    async fn preview(
        &self,
        entity: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<String> {
        from_intentd(connection, &header).await?;
        render(&self.provider.preview(&parse(&entity)?).await)
    }

    async fn suggest(
        &self,
        ask: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<String> {
        from_intentd(connection, &header).await?;
        render(&self.provider.suggest(parse(&ask)?).await)
    }

    // The ids-then-metas split has no provider method yet: nothing resolves keys. (A plain
    // comment: a doc comment would land in the introspection.)
    async fn resolve(&self, keys: String) -> fdo::Result<String> {
        let _ = keys;
        Err(fdo::Error::NotSupported(
            "this provider resolves no keys".into(),
        ))
    }

    #[zbus(signal)]
    async fn undo_changed(
        emitter: &SignalEmitter<'_>,
        token: &str,
        state: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn index_stale(emitter: &SignalEmitter<'_>, epoch: u64) -> zbus::Result<()>;
}

/// Exports `provider` at `/org/quire/IntentProvider1` on `connection` and claims the manifest's
/// app as its bus name. Returns once the name is ours; the connection keeps serving.
pub async fn serve_on<P, C, S>(
    connection: &BusConnection,
    provider: P,
    context: C,
    summon: S,
) -> Result<(), TransportError>
where
    P: IntentProvider + 'static,
    C: ContextSource + 'static,
    S: SummonTarget + 'static,
{
    let manifest: &ValidManifest = provider.manifest();
    let name = manifest.manifest().app.to_string();
    let object = ProviderObject {
        provider,
        context,
        summon,
    };
    let bus = |e: zbus::Error| TransportError::Bus(e.to_string());
    connection
        .object_server()
        .at(PROVIDER_PATH, object)
        .await
        .map_err(bus)?;
    let reply = connection
        .request_name_with_flags(name.as_str(), fdo::RequestNameFlags::DoNotQueue.into())
        .await
        .map_err(bus)?;
    match reply {
        fdo::RequestNameReply::PrimaryOwner | fdo::RequestNameReply::AlreadyOwner => Ok(()),
        fdo::RequestNameReply::InQueue | fdo::RequestNameReply::Exists => {
            Err(TransportError::Bus(format!("{name} is taken")))
        }
    }
}

/// Returns when the connection closes.
pub(crate) async fn until_closed(connection: &BusConnection) {
    let mut messages = zbus::MessageStream::from(connection);
    while messages.next().await.is_some() {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use docket_core::{
        AppRefusal, ContextScope, ContextSnapshot, EntityRef, Hit, Invocation, Outcome, Preview,
        SuggestAsk, SummonAnswer, SummonOrigin, UndoFault, UndoToken,
    };
    use prov::{Actor, EntityId};
    use zbus::object_server::Interface;

    struct Nobody(ValidManifest);

    impl IntentProvider for Nobody {
        fn manifest(&self) -> &ValidManifest {
            &self.0
        }
        async fn perform(&self, _: Invocation) -> Result<Outcome, AppRefusal> {
            Err(AppRefusal::Unsupported)
        }
        async fn dry_run(&self, _: Invocation) -> Result<Preview, AppRefusal> {
            Err(AppRefusal::Unsupported)
        }
        async fn undo(&self, _: UndoToken, _: Actor) -> Result<(), UndoFault> {
            Err(UndoFault::Gone)
        }
        async fn search(&self, _: &str) -> Vec<Hit> {
            vec![]
        }
        async fn preview(&self, _: &EntityId) -> Preview {
            Preview::None
        }
        async fn suggest(&self, _: SuggestAsk) -> Vec<EntityRef> {
            vec![]
        }
    }

    struct Blank;
    impl ContextSource for Blank {
        fn snapshot(&self, _: ContextScope) -> ContextSnapshot {
            unreachable!("introspection only")
        }
    }
    impl SummonTarget for Blank {
        fn summon(&self, _: SummonSerial, _: SummonOrigin) -> SummonAnswer {
            unreachable!("introspection only")
        }
    }

    #[test]
    fn the_served_interface_is_the_declared_one() {
        let manifest = docket_core::validate(docket_core::Manifest {
            vocab: docket_core::IntentsVocab(1),
            app: prov::AppName::parse("org.quire.Nobody").expect("an app name"),
            entities: vec![],
            actions: vec![],
            visibility: docket_core::Visibility::Everyone,
        })
        .expect("a manifest");
        let object = ProviderObject {
            provider: Nobody(manifest),
            context: Blank,
            summon: Blank,
        };
        let mut served = String::from(
            "<!DOCTYPE node PUBLIC \"-//freedesktop//DTD D-BUS Object Introspection 1.0//EN\"\n \"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd\">\n<node>\n",
        );
        object.introspect_to_writer(&mut served, 1);
        served.push_str("</node>\n");
        let declared = include_str!("../../../dbus/org.quire.IntentProvider1.xml");
        assert_eq!(served, declared);
    }
}
