//! `Voice1.Prepare`: warms the speech-to-text engine the route would pick, with no mic and no
//! request, and says how ready it is. The shipped warmer asks inferd-client's
//! `Transport::prepare`; a test hands in a fixed answer or a scripted transport.

use inferd_client::{DbusTransport, OpenOptions, Transport};
use porter_capability::SpeechMode;
use porter_capability::Tier;
use porter_core::DataClass;
use porter_core::consent::Usage;
use porter_infer::Readiness;
use porter_match::Need;
use porter_match::SpeechNeed;
use std::collections::BTreeSet;
use std::future::Future;

/// The speech-to-text need: the one thing voiced asks of inferd for hearing.
pub(crate) fn stt_need() -> Need {
    Need::Speech(SpeechNeed::new(BTreeSet::from([SpeechMode::Stt])))
}

/// The text-to-speech need.
pub(crate) fn tts_need() -> Need {
    Need::Speech(SpeechNeed::new(BTreeSet::from([SpeechMode::Tts])))
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

/// Warms through inferd-client's [`Transport::prepare`]: `Inference1.Prepare` when the
/// transport is the session bus. A refusal (`Denied`) and an absent inferd (`Unreachable`) both
/// read as `Unavailable`, so the shell sees one answer for "cannot hear now".
#[derive(Debug, Clone)]
pub struct TransportWarm<T> {
    transport: T,
    tier: Tier,
}

/// The shipped warmer: inferd-client's D-Bus transport on the session bus.
pub type BusWarm = TransportWarm<DbusTransport>;

impl TransportWarm<DbusTransport> {
    /// Warms over `connection` for `tier`.
    pub fn new(connection: zbus::Connection, tier: Tier) -> Self {
        Self::over(DbusTransport::over(connection), tier)
    }
}

impl<T: Transport> TransportWarm<T> {
    /// Warms through `transport` for `tier`.
    pub fn over(transport: T, tier: Tier) -> Self {
        Self { transport, tier }
    }
}

impl<T: Transport + 'static> Warm for TransportWarm<T> {
    async fn warm(&self) -> Readiness {
        self.transport
            .prepare(
                &stt_need(),
                DataClass::Voice,
                self.tier,
                &OpenOptions::default().with_usage(Usage::Interactive),
            )
            .await
            .unwrap_or(Readiness::Unavailable)
    }
}
