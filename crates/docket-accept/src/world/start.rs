//! Starting a world, in dependency order.

use super::{
    AcpSetting, Binaries, Cassette, Cgroup, Consent, InferdEnv, MEMORY_CALLERS, ModelSource,
    Options, Record, SPACES, Scratch, TapMode, World, copy_catalog, inferd_toml, place, spawn,
    within, write, write_system_callers,
};
use crate::confirm;
use crate::provider::{AcceptMail, QuietWindow};
use crate::things::{AcceptThings, App};
use docket_core::{ValidManifest, parse};
use docket_dbus::callers::Identity;
use docket_testbus::PrivateBus;
use futures_util::StreamExt;
use std::path::Path;
use zbus::fdo::DBusProxy;

/// Who the app's provider answers: the process the scratch root's fake proc has as intentd's unit.
fn intentd_only(connection: &zbus::Connection, root: &Path) -> std::sync::Arc<Identity> {
    std::sync::Arc::new(Identity::on_fixture(
        connection.clone(),
        docket_dbus::callers::intentd_only(),
        root.join("proc"),
    ))
}

/// Waits until `name` has an owner on the bus. Subscribes first, so an owner that appears
/// between the check and the wait is not missed.
pub async fn until_owned(connection: &zbus::Connection, name: &str) {
    let dbus = DBusProxy::new(connection).await.expect("dbus");
    let mut changes = dbus
        .receive_name_owner_changed_with_args(&[(0, name)])
        .await
        .expect("subscribed");
    let bus_name = zbus::names::BusName::try_from(name).expect("name");
    let wait = async {
        while !dbus.name_has_owner(bus_name.clone()).await.unwrap_or(false) {
            changes.next().await;
        }
    };
    within(&format!("{name} to appear on the bus"), wait).await;
}

impl World {
    /// Starts everything, in dependency order: the bus, sill, inferd (replaying `cassette`),
    /// memoryd, intentd (with the mail manifest installed), the mail provider, readerd, companiond.
    pub async fn start(binaries: &Binaries, consent: Consent, cassette: Cassette) -> World {
        let model = ModelSource::Scripted(cassette.0.to_owned());
        World::start_model(binaries, consent, &model, &Options::default()).await
    }

    /// [`World::start`] with the person's `agent.acp.expose` switched on, for a run with an editor.
    pub async fn start_acp(binaries: &Binaries, consent: Consent, cassette: Cassette) -> World {
        let model = ModelSource::Scripted(cassette.0.to_owned());
        let options = Options {
            acp: AcpSetting::On,
            ..Options::default()
        };
        World::start_model(binaries, consent, &model, &options).await
    }

    /// [`World::start`] with any model source and options: a cassette or a real engine, the
    /// scratch root kept or not, the daemons' model tap on or off.
    pub async fn start_model(
        binaries: &Binaries,
        consent: Consent,
        model: &ModelSource,
        options: &Options,
    ) -> World {
        let dir = Scratch::made(options.keep_in.as_deref()).expect("scratch");
        let root = dir.path();
        for sub in ["logs", "data", "config", "cache", "run"] {
            std::fs::create_dir_all(root.join(sub)).expect("dirs");
        }
        std::fs::set_permissions(
            root.join("run"),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .expect("run dir mode");
        crate::runlink::link_run(root).expect("short runtime name");
        let bus = PrivateBus::start(root);
        let address = bus.address().to_owned();

        // sill is on the bus before any daemon needs it.
        let sill = within("a connection to the private bus", bus.connect()).await;
        let (sheet, confirms) = within("sill's Confirm1 to be served", confirm::serve(&sill))
            .await
            .expect("Confirm1");

        // Files the daemons read: memoryd's callers, its Spaces, intentd's manifest and consent.
        // The test process is the shell (sill): the fake proc root says so.
        let record = match options.record {
            Record::On => Some(root.join("record.jsonl")),
            Record::Off => None,
        };
        place(root, std::process::id(), Cgroup::Unit("sill"));
        write(
            &root.join("config/quire/memory-callers.toml"),
            MEMORY_CALLERS,
        );
        write(&root.join("data/quire/memory/spaces.toml"), SPACES);
        write_system_callers(root).expect("the system callers files");
        if let ModelSource::Scripted(cassette) = model {
            write(&root.join("cassette.jsonl"), cassette);
        }
        write(
            &root.join("config/quire/inferd.toml"),
            &inferd_toml(root, model, record.as_deref()),
        );
        write(
            &root.join(format!("data/quire/intents/{}.toml", options.app.name())),
            options.app.manifest(),
        );
        if consent == Consent::Standing {
            write(
                &root.join("data/quire/intents/grants.json"),
                &crate::grants::standing_json("work", options.app),
            );
        }

        write(
            &root.join("config/docket/settings.toml"),
            &settings_toml(options.acp),
        );
        copy_catalog(options.catalog.as_deref(), root);
        let tap = match options.tap {
            TapMode::On => Some(root.join("model.jsonl")),
            TapMode::Off => None,
        };
        let keys = options
            .keys_home
            .as_ref()
            .map(crate::keys_home::KeysHome::keys_setting);
        let inferd = InferdEnv {
            keys: keys.as_deref(),
            engine_cache: binaries.engine_cache.as_deref(),
        };
        let mut daemons = Vec::new();
        let mut start = async |name: &'static str, binary: &Path, owns: &str| {
            let daemon =
                spawn(root, &address, name, binary, tap.as_deref(), inferd).expect("daemon starts");
            place(root, daemon.1.pid(), Cgroup::Unit(name));
            daemons.push(daemon);
            until_owned(&sill, owns).await;
        };
        if let Some(home) = &options.keys_home {
            home.seed(root).expect("the key home's records are copied");
        }
        start("inferd", &binaries.inferd, "org.quire.Inference1").await;
        start("memoryd", &binaries.memoryd, "org.quire.Memory1").await;
        start("intentd", &binaries.intentd, "org.quire.Intents1").await;

        // The app's provider: its own connection, its own name, the manifest's wire.
        let provider_connection =
            within("the provider's connection to the bus", bus.connect()).await;
        let manifest: ValidManifest = parse(options.app.manifest()).expect("the app's manifest");
        let space = prov::SpaceId::parse("work").expect("space");
        let quiet = QuietWindow {
            app: porter_core::AppName::parse(options.app.name()).expect("app"),
            focus: options.focus,
            window: options.app.window(),
        };
        let log = match options.app {
            App::Mail => {
                let (mail, log) = AcceptMail::new(manifest, space);
                let mail = mail.focused(options.focus);
                within(
                    "the mail provider to serve",
                    docket_provider::serve_on_with(
                        &provider_connection,
                        mail,
                        quiet.clone(),
                        quiet,
                        intentd_only(&provider_connection, root),
                    ),
                )
                .await
                .expect("the mail provider serves");
                log
            }
            other => {
                let (things, log) = AcceptThings::new(other, manifest, space);
                within(
                    "the app's provider to serve",
                    docket_provider::serve_on_with(
                        &provider_connection,
                        things,
                        quiet.clone(),
                        quiet,
                        intentd_only(&provider_connection, root),
                    ),
                )
                .await
                .expect("the app's provider serves");
                log
            }
        };

        start("readerd", &binaries.readerd, "org.quire.Reader1").await;
        start("companiond", &binaries.companiond, "org.quire.Companion1").await;
        World {
            daemons,
            sheet,
            confirms,
            app: log,
            sill,
            _helpers: vec![provider_connection],
            relays: std::sync::Mutex::default(),
            dir,
            bus,
        }
    }
}
