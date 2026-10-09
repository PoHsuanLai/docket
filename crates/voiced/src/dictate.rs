//! Dictation in process: one utterance from a microphone to text, over the `AudioDevice` seam and
//! any porter-client `Transport`, with no bus, no skeletons and no daemon. The caller is already
//! allowed to hear (consent is the caller's to check); the capture ends at the silence after
//! speech, or when the stream does.

use crate::device::{
    AudioDevice, CaptureFormat, CaptureStream, SNAPSHOT_BUDGET, choose_capture, snapshot_before,
};
use crate::frames::{Dictation, FRAME, Framer};
use crate::link::fault_of as link_fault;
use crate::warm::stt_need;
use porter_client::{InferSession, Transport};
use porter_core::consent::Usage;
use porter_core::{DataClass, Tier};
use porter_infer::{
    AudioFrame, AudioRate, Base64Bytes, ClientFrame, InferEvent, InferReply, InferRequest,
    LangPick, ModelError, ServedBy, TranscribeBegin, TranscribeMode,
};
use voice_loop::CAPTURE_RATE;
use voice_wire::{HeardText, VoiceFault};

/// What dictation heard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dictated {
    /// Everything that was said (redacted in Debug).
    pub text: HeardText,
    /// Who transcribed it.
    pub served: ServedBy,
}

fn pcm_bytes(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

fn engine_gone<E>(_: E) -> VoiceFault {
    VoiceFault::Engine(ModelError::Unreachable)
}

/// Listens on `input` (a source name; `None` is the device default) and returns the transcript.
pub async fn dictate<D: AudioDevice, T: Transport>(
    device: &D,
    transport: &T,
    input: Option<&str>,
) -> Result<Dictated, VoiceFault> {
    let found = snapshot_before(device, tokio::time::sleep(SNAPSHOT_BUDGET))
        .await
        .map_err(|_| VoiceFault::MicUnavailable)?;
    let node = choose_capture(&found.sources, found.default.as_deref(), input)
        .map(|chosen| chosen.node.clone())
        .ok_or(VoiceFault::MicUnavailable)?;
    let mut capture = device
        .open_capture(&node, CaptureFormat { rate: CAPTURE_RATE })
        .await
        .map_err(crate::utter::fault_of)?;
    let mut session = transport
        .open(&stt_need(), DataClass::Voice, Tier::Balanced)
        .await
        .map_err(|e| link_fault(&e))?;
    let begin = TranscribeBegin {
        mode: TranscribeMode::Streaming,
        lang: LangPick::Auto,
        rate: AudioRate(CAPTURE_RATE),
        usage: Usage::Interactive,
    };
    session
        .send(ClientFrame::Request(InferRequest::Transcribe(begin)))
        .await
        .map_err(engine_gone)?;
    listen(&mut capture, &mut session).await?;
    session
        .send(ClientFrame::EndOfAudio)
        .await
        .map_err(engine_gone)?;
    transcript(&mut session).await
}

/// Sends 512-sample frames until the silence after speech, or the end of the stream.
async fn listen<C: CaptureStream, S: InferSession>(
    capture: &mut C,
    session: &mut S,
) -> Result<(), VoiceFault> {
    let (mut dictation, mut framer, mut sent) = (Dictation::new(), Framer::default(), 0u64);
    while let Some(samples) = capture.next().await {
        framer.push(samples);
        while let Some((frame, framed)) = framer.next_frame() {
            let audio = AudioFrame {
                at: sent,
                pcm: Base64Bytes(pcm_bytes(&frame)),
            };
            sent += FRAME as u64;
            session
                .send(ClientFrame::Audio(audio))
                .await
                .map_err(engine_gone)?;
            if dictation.ended(&framed) {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Reads events until the engine finishes.
async fn transcript<S: InferSession>(session: &mut S) -> Result<Dictated, VoiceFault> {
    loop {
        match session.next().await.map_err(engine_gone)? {
            InferEvent::Finished(InferReply::Transcribed(reply)) => {
                return Ok(Dictated {
                    text: HeardText(reply.text),
                    served: reply.served,
                });
            }
            InferEvent::Finished(InferReply::Refused(why)) => return Err(VoiceFault::Refused(why)),
            InferEvent::Finished(InferReply::Failed(why)) => return Err(VoiceFault::Engine(why)),
            InferEvent::Finished(_) => return Err(VoiceFault::Engine(ModelError::Unreadable)),
            _ => {}
        }
    }
}
