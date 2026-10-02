//! `org.quire.Intents1.Index`: the shadow index apps push titles into (the owning app only).

use zbus::fdo;

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.Intents1.Index",
    default_service = "org.quire.Intents1",
    default_path = "/org/quire/Intents1"
)]
pub trait Index {
    /// Pushes a batch (`IndexBatch` JSON).
    fn push(&self, batch: &str) -> zbus::Result<()>;

    /// Starts an epoch; the next pushes belong to it.
    fn reset(&self, epoch: u64) -> zbus::Result<()>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct IndexSkeleton;

#[zbus::interface(name = "org.quire.Intents1.Index")]
impl IndexSkeleton {
    fn push(&self, batch: String) -> fdo::Result<()> {
        let _ = (batch,);
        Err(crate::introspect::frozen())
    }

    fn reset(&self, epoch: u64) -> fdo::Result<()> {
        let _ = (epoch,);
        Err(crate::introspect::frozen())
    }
}
