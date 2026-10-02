//! The summon seam: sill asks an app `IntentProvider1.Summon(serial, origin)`; ds answers
//! through its companion port. This converts the plain values both ways and holds the target
//! `docket-client` asks.

use docket_client::SummonTarget;
use docket_core::{SummonAnswer, SummonOrigin, SummonSerial};
use ds_intents::{SummonAnswerMark, SummonSerial as DsSerial};

/// ds's answer as the wire's. Total: a new variant on either side stops the build here.
pub fn summon_answer_of(mark: SummonAnswerMark) -> SummonAnswer {
    match mark {
        SummonAnswerMark::TookField => SummonAnswer::TookField,
        SummonAnswerMark::TookAnchored => SummonAnswer::TookAnchored,
        SummonAnswerMark::Restored => SummonAnswer::Restored,
        SummonAnswerMark::Declined => SummonAnswer::Declined,
    }
}

/// The wire's answer as ds's.
pub fn summon_answer_mark(answer: SummonAnswer) -> SummonAnswerMark {
    match answer {
        SummonAnswer::TookField => SummonAnswerMark::TookField,
        SummonAnswer::TookAnchored => SummonAnswerMark::TookAnchored,
        SummonAnswer::Restored => SummonAnswerMark::Restored,
        SummonAnswer::Declined => SummonAnswerMark::Declined,
    }
}

/// The wire's serial as ds's.
pub fn serial_mark(serial: SummonSerial) -> DsSerial {
    DsSerial(serial.0)
}

/// ds's serial as the wire's.
pub fn serial_of(serial: DsSerial) -> SummonSerial {
    SummonSerial(serial.0)
}

/// What ds does when a summon arrives: the host app's prompt machinery. A quire app implements
/// it over ds's `CompanionPort`; `DsSummonTarget` is generic over it so the conversion is
/// tested without a window.
pub trait PromptHost: Send + Sync {
    /// Takes the summon: the focused field becomes a prompt, an anchored prompt opens, the
    /// previous prompt returns, or the app declines.
    fn summoned(&self, serial: DsSerial, origin: &SummonOrigin) -> SummonAnswerMark;
}

/// The `SummonTarget` of a quire app.
#[derive(Debug)]
pub struct DsSummonTarget<H: PromptHost> {
    host: H,
}

impl<H: PromptHost> DsSummonTarget<H> {
    /// Answers summons through `host`.
    pub fn new(host: H) -> Self {
        Self { host }
    }
}

impl<H: PromptHost> SummonTarget for DsSummonTarget<H> {
    fn summon(&self, serial: SummonSerial, origin: SummonOrigin) -> SummonAnswer {
        summon_answer_of(self.host.summoned(serial_mark(serial), &origin))
    }
}

/// Why a voice attach did not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BridgeFault {
    /// This app is not the one the shell routed the utterance to.
    #[error("not the routed app")]
    NotRouted,
    /// The voice service is not there.
    #[error("voice service unavailable")]
    Unavailable,
}
