//! Where this computer keeps what the host needs: the configuration and data directories, the
//! home folder, `PATH`, the runtime directory and the few variables an agent's own login needs.
//! Read once from the environment the program's `main` passes in, so nothing below `main` reads
//! the environment.

use agents_core::dirs::AgentsDir;
use docket_launch::login::VisibleLogin;
use docket_settings::Locator;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The variables an agent's own login or logout command is given to show itself.
const LOGIN_VARS: [&str; 6] = [
    "HOME",
    "PATH",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
];

/// The environment `main` hands in: a variable's value, or none.
type Env = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// The places and search path of this computer, as the environment gave them.
#[derive(Clone)]
pub struct Places {
    env: Env,
    locator: Locator,
    data: PathBuf,
    home: Option<PathBuf>,
    path: String,
    runtime: Option<PathBuf>,
    login: Vec<(String, String)>,
}

impl std::fmt::Debug for Places {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Places")
            .field("data", &self.data)
            .field("home", &self.home)
            .finish_non_exhaustive()
    }
}

impl Places {
    /// Reads the places through `env`, which the places keep for the variables read later.
    pub fn read(env: impl Fn(&str) -> Option<String> + Send + Sync + 'static) -> Self {
        let env: Env = Arc::new(env);
        let get = |name: &str| (*env)(name);
        let data = get("XDG_DATA_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(get("HOME").unwrap_or_default()).join(".local/share"))
            .join("docket");
        Self {
            locator: Locator::from_env(&get),
            data,
            home: get("HOME").map(PathBuf::from),
            path: get("PATH").unwrap_or_default(),
            runtime: get("XDG_RUNTIME_DIR").map(PathBuf::from),
            login: LOGIN_VARS
                .iter()
                .filter_map(|k| get(k).map(|v| ((*k).to_owned(), v)))
                .collect(),
            env,
        }
    }

    /// The value of the variable `name`, as `main` handed the environment in.
    pub fn var(&self, name: &str) -> Option<String> {
        (*self.env)(name)
    }

    /// The settings locator.
    pub fn locator(&self) -> &Locator {
        &self.locator
    }

    /// The first configuration directory (the person's own).
    pub fn config_dir(&self) -> PathBuf {
        self.locator.dirs().first().cloned().unwrap_or_default()
    }

    /// docket's data directory.
    pub fn data_dir(&self) -> &Path {
        &self.data
    }

    /// Where agents from the registry are installed (`docket-agents --dir`): `agents` in the
    /// data directory.
    pub fn agents_dir(&self) -> AgentsDir {
        AgentsDir::at(&self.data.join("agents"))
    }

    /// The home folder, when the environment names one.
    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    /// The program search path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The runtime directory only the person can enter, when the environment names one.
    pub fn runtime_dir(&self) -> Option<&Path> {
        self.runtime.as_deref()
    }

    /// What runs an agent's own login or logout command: given the variables it needs to show
    /// itself, chosen here because the launcher crate reads no environment.
    pub fn login_runner(&self) -> VisibleLogin {
        VisibleLogin::new(self.login.clone())
    }
}
