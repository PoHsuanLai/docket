//! The summon seam: sill asks an app `IntentProvider1.Summon(serial, origin)`; ds answers
//! through its companion port. This converts the plain values both ways and holds the target
//! `docket-client` asks.

use docket_client::SummonTarget;
use docket_core::{SummonAnswer, SummonOrigin, SummonSerial, VoiceIntent};
use ds_intents::{SummonAnswerMark, SummonOriginMark, SummonSerial as DsSerial};

/// ds's answer as the wire's. A new wire variant stops the build in the other direction; an unknown ds mark reads as declined.
pub fn summon_answer_of(mark: SummonAnswerMark) -> SummonAnswer {
    match mark {
        SummonAnswerMark::TookField => SummonAnswer::TookField,
        SummonAnswerMark::TookAnchored => SummonAnswer::TookAnchored,
        SummonAnswerMark::Restored => SummonAnswer::Restored,
        SummonAnswerMark::Declined => SummonAnswer::Declined,
        // A mark newer than this build knows: the field was not taken.
        _ => SummonAnswer::Declined,
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

/// Where a summon came from, as ds's view of it: the double-tap is a key, and a voice summon is
/// a prompt (`Ask`) or a dictation (`Dictate`). Total.
pub fn summon_origin_mark(origin: &SummonOrigin) -> SummonOriginMark {
    match origin {
        SummonOrigin::DoubleTap => SummonOriginMark::Keyboard,
        SummonOrigin::Voice {
            intent: VoiceIntent::Ask,
            ..
        } => SummonOriginMark::Voice,
        SummonOrigin::Voice {
            intent: VoiceIntent::Dictate,
            ..
        } => SummonOriginMark::Dictation,
    }
}

/// What ds does when a summon arrives: the host app's prompt machinery. A quire app implements
/// it over ds's `CompanionPort`; `DsSummonTarget` is generic over it so the conversion is
/// tested without a window.
pub trait PromptHost: Send + Sync {
    /// Takes the summon: the focused field becomes a prompt, an anchored prompt opens, the
    /// previous prompt returns, or the app declines.
    fn summoned(&self, serial: DsSerial, origin: SummonOriginMark) -> SummonAnswerMark;
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
        summon_answer_of(
            self.host
                .summoned(serial_mark(serial), summon_origin_mark(&origin)),
        )
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
