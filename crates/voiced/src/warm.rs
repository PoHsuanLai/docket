//! `Voice1.Prepare`: warms the speech-to-text engine the route would pick, with no mic and no
//! request, and says how ready it is. porter-client's `Transport` has no `prepare` yet (interface
//! ask), so the shipped warmer calls `Inference1.Prepare` through porter-dbus; a test hands in a
//! fixed answer.

use porter_core::capability::SpeechMode;
use porter_core::need::SpeechNeed;
use porter_core::{DataClass, Need, Tier};
use porter_dbus::{Details, InferenceProxy, need_to_dbus};
use porter_infer::Readiness;
use std::collections::BTreeSet;
use std::future::Future;

/// The speech-to-text need: the one thing voiced asks of inferd for hearing.
pub(crate) fn stt_need() -> Need {
    Need::Speech(SpeechNeed {
        modes: BTreeSet::from([SpeechMode::Stt]),
    })
}

/// The text-to-speech need.
pub(crate) fn tts_need() -> Need {
    Need::Speech(SpeechNeed {
        modes: BTreeSet::from([SpeechMode::Tts]),
    })
}

/// How hot the engine is.
pub trait Warm: Send + Sync + 'static {
    /// Asks inferd to warm the speech-to-text engine; the readiness it reports.
    fn warm(&self) -> impl Future<Output = Readiness> + Send;
}

/// A fixed answer.
#[derive(Debug, Clone, Copy)]
pub struct FixedWarm(pub Readiness);

impl Warm for FixedWarm {
    async fn warm(&self) -> Readiness {
        self.0
    }
}

/// `Inference1.Prepare` on the session bus.
#[derive(Debug, Clone)]
pub struct BusWarm {
    connection: zbus::Connection,
    tier: Tier,
}

impl BusWarm {
    /// Warms over `connection` for `tier`.
    pub fn new(connection: zbus::Connection, tier: Tier) -> Self {
        Self { connection, tier }
    }
}

fn slug<T: serde::Serialize>(value: &T) -> Option<String> {
    match serde_json::to_value(value).ok()? {
        serde_json::Value::String(text) => Some(text),
        _ => None,
    }
}

fn readiness_of(slug: &str) -> Readiness {
    match slug {
        "ready" => Readiness::Ready,
        "loading" => Readiness::Loading,
        "loadable" => Readiness::Loadable,
        "downloadable" => Readiness::Downloadable,
        _ => Readiness::Unavailable,
    }
}

impl Warm for BusWarm {
    async fn warm(&self) -> Readiness {
        let (Some(class), Some(tier)) = (slug(&DataClass::Voice), slug(&self.tier)) else {
            return Readiness::Unavailable;
        };
        let need = need_to_dbus(&stt_need());
        let Ok(proxy) = InferenceProxy::new(&self.connection).await else {
            return Readiness::Unavailable;
        };
        proxy
            .prepare(&need, &class, &tier, &Details::new())
            .await
            .map_or(Readiness::Unavailable, |text| readiness_of(&text))
    }
}
