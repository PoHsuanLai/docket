//! `org.quire.Intents1.Gate`: the computer-use gate, for cuad.

use porter_dbus::Details;
use zbus::fdo;
use zbus::zvariant::OwnedObjectPath;

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.Intents1.Gate",
    default_service = "org.quire.Intents1",
    default_path = "/org/quire/Intents1"
)]
pub trait Gate {
    /// Asks for the per-app consent (`GrantAsk` JSON: the app, the Space, the caller's own level
    /// and the run it is for). Answers a Request whose `Response` carries a `GrantAnswer`.
    fn grant(&self, ask: &str) -> zbus::Result<OwnedObjectPath>;

    /// Checks one pixel step (`CuaAsk` JSON). Answers a Request whose `Response` carries a `GateAnswer`.
    fn check(&self, ask: &str, options: &Details) -> zbus::Result<OwnedObjectPath>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct GateSkeleton;

#[zbus::interface(name = "org.quire.Intents1.Gate")]
impl GateSkeleton {
    fn grant(&self, ask: String) -> fdo::Result<OwnedObjectPath> {
        let _ = ask;
        Err(crate::introspect::frozen())
    }

    fn check(&self, ask: String, options: Details) -> fdo::Result<OwnedObjectPath> {
        let _ = (ask, options);
        Err(crate::introspect::frozen())
    }
}
