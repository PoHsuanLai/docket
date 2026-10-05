//! A private session bus for one test: a `dbus-daemon` made from a scratch config in a scratch
//! directory, with nothing from the environment. The person's real bus is never named.
//!
//! Teardown is certain. The daemon runs with `--nofork`, so the process this crate spawns IS the
//! daemon and its PID is the daemon's (never a shell's). `PrivateBus` owns it through a
//! [`Reaped`] guard that is made before anything else can fail, so the daemon is killed by PID and
//! waited for when the bus drops: at the end of the test, on a panic, on an early return. And if
//! the test process itself dies without unwinding (a timeout's SIGKILL), the guard's watchdog
//! kills the daemon by PID once it sees the test process gone. No kill here goes by a name or a
//! pattern.

mod guard;

pub use guard::Reaped;

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

const CONFIG: &str = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:path=SOCKET</listen>
  <auth>EXTERNAL</auth>
  SERVICEDIR
  <policy context="default">
    RULES
  </policy>
</busconfig>
"#;

/// What the bus's default policy lets a connection do beyond sending, receiving and owning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Policy {
    Plain,
    Eavesdrop,
}

impl Policy {
    fn rules(self) -> &'static str {
        match self {
            Self::Plain => {
                r#"<allow send_destination="*"/>
    <allow receive_sender="*"/>
    <allow own="*"/>"#
            }
            Self::Eavesdrop => {
                r#"<allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>"#
            }
        }
    }
}

/// A bus of our own, killed when the test ends.
#[derive(Debug)]
pub struct PrivateBus {
    // Declared first: the daemon is killed and waited for before anything else of ours goes.
    daemon: Reaped,
    address: String,
}

impl PrivateBus {
    /// A bus in `dir`.
    pub fn start(dir: &Path) -> Self {
        Self::launch(dir, Policy::Plain, None, &[])
    }

    /// A bus that also activates the services described in `servicedir`, and whose own
    /// environment (what an activated service inherits) is `env`.
    pub fn start_with(dir: &Path, servicedir: Option<&Path>, env: &[(&str, &str)]) -> Self {
        Self::launch(dir, Policy::Plain, servicedir, env)
    }

    /// A bus whose connections may eavesdrop (for a test that watches what a binary sends).
    pub fn start_eavesdropping(dir: &Path) -> Self {
        Self::launch(dir, Policy::Eavesdrop, None, &[])
    }

    fn launch(dir: &Path, policy: Policy, servicedir: Option<&Path>, env: &[(&str, &str)]) -> Self {
        let socket = dir.join("bus.sock");
        let config = dir.join("bus.conf");
        let services = servicedir
            .map(|d| format!("<servicedir>{}</servicedir>", d.display()))
            .unwrap_or_default();
        let text = CONFIG
            .replace("RULES", policy.rules())
            .replace("SOCKET", &socket.to_string_lossy())
            .replace("SERVICEDIR", &services);
        std::fs::write(&config, text).expect("bus config");
        let mut daemon = Reaped::spawn(
            Command::new("dbus-daemon")
                .arg(format!("--config-file={}", config.display()))
                .args(["--nofork", "--print-address=1"])
                .env_clear()
                .envs(env.iter().copied())
                .stdout(Stdio::piped())
                .stderr(Stdio::null()),
        )
        .expect("dbus-daemon is installed");
        // From here the guard owns the daemon: a failure below unwinds through its `Drop`.
        let stdout = daemon.child_mut().stdout.take().expect("stdout");
        let mut address = String::new();
        BufReader::new(stdout)
            .read_line(&mut address)
            .expect("the daemon prints its address");
        assert!(
            address.starts_with("unix:"),
            "the daemon printed no address: {address:?}"
        );
        Self {
            daemon,
            address: address.trim().to_owned(),
        }
    }

    /// The address a daemon started by the test is told to use.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// The PID of the `dbus-daemon` itself.
    pub fn pid(&self) -> u32 {
        self.daemon.pid()
    }

    /// A new connection to this bus.
    pub async fn connect(&self) -> docket_dbus::BusConnection {
        zbus::connection::Builder::address(self.address.as_str())
            .expect("address")
            .build()
            .await
            .expect("connect to the private bus")
    }
}
