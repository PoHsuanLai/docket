//! A private session bus for one test: a `dbus-daemon` made from a scratch config in a scratch
//! directory, with nothing from the environment. The person's real bus is never named.
#![allow(dead_code)]

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};

const CONFIG: &str = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:path=SOCKET</listen>
  <auth>EXTERNAL</auth>
  SERVICEDIR
  <policy context="default">
    <allow send_destination="*"/>
    <allow receive_sender="*"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#;

/// A bus of our own, killed when the test ends.
pub struct PrivateBus {
    child: Child,
    address: String,
}

impl PrivateBus {
    /// The address of this bus.
    pub fn address(&self) -> &str {
        &self.address
    }

    pub fn start(dir: &Path) -> Self {
        Self::start_with(dir, None, &[])
    }

    /// A bus that also activates the services described in `servicedir`, and whose own
    /// environment (what an activated service inherits) is `env`.
    pub fn start_with(dir: &Path, servicedir: Option<&Path>, env: &[(&str, &str)]) -> Self {
        let socket = dir.join("bus.sock");
        let config = dir.join("bus.conf");
        let services = servicedir
            .map(|d| format!("<servicedir>{}</servicedir>", d.display()))
            .unwrap_or_default();
        std::fs::write(
            &config,
            CONFIG
                .replace("SOCKET", &socket.to_string_lossy())
                .replace("SERVICEDIR", &services),
        )
        .expect("bus config");
        let mut child = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .env_clear()
            .envs(env.iter().copied())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("dbus-daemon is installed");
        let mut address = String::new();
        BufReader::new(child.stdout.take().expect("stdout"))
            .read_line(&mut address)
            .expect("the daemon prints its address");
        Self {
            child,
            address: address.trim().to_owned(),
        }
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

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
