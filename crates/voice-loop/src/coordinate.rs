//! Running the two machines for one `Begin`: speech stops before the mic opens.

use crate::speech::{SpeechEffect, SpeechEvent, SpeechState, speech_step};
use crate::utterance::{UtteranceEffect, UtteranceEvent, UtteranceState, mic_of, utterance_step};
use serde::{Deserialize, Serialize};

/// One effect, tagged with the machine that produced it. The daemon runs them in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Step {
    /// From the speech machine.
    Speech(SpeechEffect),
    /// From the utterance machine.
    Utterance(UtteranceEffect),
}

/// A `Begin`: any speech is barged in on first (`StopSynth` precedes `OpenMic`), then the
/// utterance machine runs, then speech learns the mic state.
pub fn begin_utterance(
    speech: SpeechState,
    utterance: UtteranceState,
    begin: UtteranceEvent,
) -> (SpeechState, UtteranceState, Vec<Step>) {
    let (speech, stopped) = speech_step(speech, SpeechEvent::Begin);
    let (utterance, started) = utterance_step(utterance, begin);
    let (speech, told) = speech_step(speech, SpeechEvent::Mic(mic_of(utterance)));
    let steps = stopped
        .into_iter()
        .map(Step::Speech)
        .chain(started.into_iter().map(Step::Utterance))
        .chain(told.into_iter().map(Step::Speech))
        .collect();
    (speech, utterance, steps)
}
