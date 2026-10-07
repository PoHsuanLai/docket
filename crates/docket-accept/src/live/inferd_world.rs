//! The smallest world a corpus run needs: a private bus and the packaged inferd, and this
//! process placed in the fake proc root as `intentd.service` so inferd serves it as
//! `org.quire.Intents` (the policy writer and the reviewers). Everything is scratch: HOME, every
//! XDG directory, the bus. The model is a cassette or the owner's own `inferd.toml`.
//!
//! The corpus cases play the router in this process (a hijacked planner mints handles in router
//! state, which the bus has no way to do), so intentd, readerd, companiond and memoryd are not
//! started here; `dev/live-smoke.sh` runs all of them.

use crate::world::{
    ACCOUNTD_CALLERS, Binaries, Cgroup, GIVE_UP, ModelSource, Options, Scratch, env_of,
    inferd_toml, place, spawn, until_owned, write,
};
use docket_dbus::BusConnection;
use docket_testbus::{PrivateBus, Reaped};
use std::path::{Path, PathBuf};

/// The running world.
pub struct InferdWorld {
    // Declared first: inferd is killed before the bus it talks on goes.
    inferd: Reaped,
    _accountd: Option<Reaped>,
    /// The scratch root.
    pub dir: Scratch,
    bus: PrivateBus,
}

impl std::fmt::Debug for InferdWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InferdWorld")
    }
}

impl InferdWorld {
    /// Starts the bus and inferd (replaying or live, as `model` says) and waits for inferd's name.
    pub async fn start(binaries: &Binaries, model: &ModelSource, options: &Options) -> Self {
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
        crate::world::copy_catalog(options.catalog.as_deref(), root);
        let bus = PrivateBus::start(root);
        let client = bus.connect().await;
        place(root, std::process::id(), Cgroup::Unit("intentd"));
        if let ModelSource::Scripted(cassette) = model {
            write(&root.join("cassette.jsonl"), cassette);
        }
        write(
            &root.join("config/quire/inferd.toml"),
            &inferd_toml(root, model, None),
        );
        let accountd = match &options.accountd {
            Some(binary) => {
                write(&root.join("config/porter/callers.toml"), ACCOUNTD_CALLERS);
                let (name, daemon) =
                    spawn(root, bus.address(), "accountd", binary, None).expect("accountd starts");
                place(root, daemon.pid(), Cgroup::Unit(name));
                until_owned(&client, "org.quire.Accounts1").await;
                Some(daemon)
            }
            None => None,
        };
        let (name, inferd) =
            spawn(root, bus.address(), "inferd", &binaries.inferd, None).expect("inferd starts");
        place(root, inferd.pid(), Cgroup::Unit(name));
        until_owned(&client, "org.quire.Inference1").await;
        let world = Self {
            inferd,
            _accountd: accountd,
            dir,
            bus,
        };
        if let Some(binary) = &options.accountd {
            world.wait_for_key(binary).await;
        }
        world
    }

    /// A cloud run's one manual step: the key goes into accountd, typed by the owner at accountd's
    /// own prompt with echo off. This process never sees it. Prints the command and waits for Enter.
    async fn wait_for_key(&self, accountd: &Path) {
        let env: Vec<String> = self
            .env()
            .into_iter()
            .chain([(
                "ACCOUNTD_KEYS".to_owned(),
                format!("file:{}", self.root().join("keys/accountd.keys").display()),
            )])
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        eprintln!(
            "docket-live: accountd runs on the private bus. Add the key in another terminal (echo is off):\n  env -i PATH=\"$PATH\" {} {} add openrouter --allow org.quire.Intents --allow org.quire.Companion --allow org.quire.Reader\nthen press Enter here.",
            env.join(" "),
            accountd.display()
        );
        let _ = tokio::task::spawn_blocking(|| {
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)
        })
        .await;
    }

    /// A fresh connection to the private bus: the corpus run's link to inferd.
    pub async fn connect(&self) -> BusConnection {
        self.bus.connect().await
    }

    /// The scratch root.
    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    /// inferd's standard error, for a failing run to print.
    pub fn log(&self) -> String {
        std::fs::read_to_string(self.root().join("logs/inferd.log")).unwrap_or_default()
    }

    /// What the models have cost so far, in micro-dollars, from inferd's own ledger (money and
    /// token counts, never content): the largest daily total of any scope, which is the whole
    /// spend because every request is counted once on its account and once on its app.
    pub fn spent_micro_usd(&self) -> u64 {
        let file: PathBuf = self.root().join(".local/state/quire/inferd/spend.json");
        let Ok(text) = std::fs::read_to_string(file) else {
            return 0;
        };
        let Ok(doc) = serde_json::from_str::<serde_json::Value>(&text) else {
            return 0;
        };
        doc["rows"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|row| row["period"] == "daily")
            .filter_map(|row| row["spent"]["micro_usd"].as_u64())
            .max()
            .unwrap_or(0)
    }

    /// How long a wait may take before a run fails instead of hanging.
    pub fn give_up() -> std::time::Duration {
        GIVE_UP
    }

    /// The inferd process id (for the owner's `ps`).
    pub fn inferd_pid(&self) -> u32 {
        self.inferd.pid()
    }

    /// `env` for a command that should talk to this world's bus as the owner's own shell
    /// would: scratch HOME and XDG directories and the private bus address.
    pub fn env(&self) -> Vec<(String, String)> {
        let mut env: Vec<(String, String)> = env_of(self.root())
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect();
        env.push((
            "DBUS_SESSION_BUS_ADDRESS".to_owned(),
            self.bus.address().to_owned(),
        ));
        env
    }
}
