//! The shipped warmer asks `Transport::prepare` for the speech-to-text need and maps its outcomes.

use crate::support::infer::{Opened, ScriptedInfer};
use porter_client::TransportError;
use porter_core::capability::SpeechMode;
use porter_core::need::SpeechNeed;
use porter_core::{DataClass, Need, Tier};
use porter_infer::Readiness;
use std::collections::BTreeSet;
use voiced::{TransportWarm, Warm};

async fn warm_with(answer: Option<Result<Readiness, TransportError>>) -> (Readiness, Vec<Opened>) {
    let infer = match answer {
        Some(answer) => ScriptedInfer::new(vec![]).prepared(answer),
        None => ScriptedInfer::new(vec![]),
    };
    let seen = infer.seen.clone();
    let readiness = TransportWarm::over(infer, Tier::Balanced).warm().await;
    (readiness, seen.opens())
}

#[tokio::test]
async fn readiness_passes_through() {
    for readiness in [
        Readiness::Ready,
        Readiness::Loading,
        Readiness::Loadable,
        Readiness::Downloadable,
        Readiness::Unavailable,
    ] {
        assert_eq!(warm_with(Some(Ok(readiness))).await.0, readiness);
    }
}

#[tokio::test]
async fn refusal_and_missing_inferd_read_as_unavailable() {
    let refused = TransportError::Denied("inferd refused: no_grant".into());
    assert_eq!(
        warm_with(Some(Err(refused))).await.0,
        Readiness::Unavailable
    );
    assert_eq!(warm_with(None).await.0, Readiness::Unavailable);
}

#[tokio::test]
async fn asks_for_voice_speech_to_text_at_the_tier() {
    let (_, asked) = warm_with(Some(Ok(Readiness::Ready))).await;
    assert_eq!(
        asked,
        vec![Opened {
            need: Need::Speech(SpeechNeed::new(BTreeSet::from([SpeechMode::Stt]))),
            class: DataClass::Voice,
            tier: Tier::Balanced,
        }]
    );
}
