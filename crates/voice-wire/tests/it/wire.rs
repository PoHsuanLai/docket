//! Every wire type survives JSON; the forms are pinned; the status holds no content.

use docket_core::{UtteranceId, VoiceIntent};
use porter_core::capability::LanguageTag;
use porter_core::{
    AccountId, AppName, DataClass, Locality, ModelId, Permille, SpaceId, UnixSeconds,
};
use porter_infer::{InferRefusal, ModelError, Readiness, ServedBy};
use serde::{Serialize, de::DeserializeOwned};
use voice_wire::*;

fn round<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(v: &T) -> String {
    let json = serde_json::to_string(v).expect("json");
    let back: T = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{json}: {e}"));
    assert_eq!(&back, v, "{json}");
    json
}

fn served() -> ServedBy {
    ServedBy {
        account: AccountId::parse("local").expect("a"),
        model: ModelId::parse("nemotron").expect("m"),
        locality: Locality::OnDevice,
    }
}

#[test]
fn voice_event_round_trip() {
    let events = vec![
        VoiceEvent::Opened,
        VoiceEvent::Waiting(Readiness::Downloading(Permille(420))),
        VoiceEvent::Level(Level(731)),
        VoiceEvent::Partial(HeardTail {
            text: HeardText("hel".into()),
        }),
        VoiceEvent::Committed(HeardSegment {
            text: HeardText("hello".into()),
        }),
        VoiceEvent::Ended(UtteranceEnd::NothingHeard),
    ];
    for e in &events {
        round(e);
    }
    assert_eq!(round(&events[0]), r#"{"kind":"opened"}"#);
    assert_eq!(round(&events[2]), r#"{"kind":"level","v":731}"#);
    assert_eq!(
        round(&events[4]),
        r#"{"kind":"committed","v":{"text":"hello"}}"#
    );
}

#[test]
fn utterance_end_round_trip() {
    let ends = vec![
        UtteranceEnd::Heard {
            text: HeardText("forward the receipts".into()),
            served: served(),
        },
        UtteranceEnd::NothingHeard,
        UtteranceEnd::Cancelled(CancelCause::Escape),
        UtteranceEnd::Failed(VoiceFault::MicDenied),
        UtteranceEnd::Failed(VoiceFault::Engine(ModelError::NotReady)),
        UtteranceEnd::Failed(VoiceFault::Refused(InferRefusal::RequiresCloud(
            DataClass::Voice,
        ))),
    ];
    for e in &ends {
        round(e);
    }
    assert_eq!(round(&ends[2]), r#"{"kind":"cancelled","v":"escape"}"#);
    assert_eq!(
        round(&ends[3]),
        r#"{"kind":"failed","v":{"kind":"mic_denied"}}"#
    );
}

#[test]
fn begin_target_and_speak_round_trip() {
    let begin = VoiceBegin {
        intent: VoiceIntent::Dictate,
        trigger: VoiceTrigger::DictationMenu,
        space: SpaceId::parse("work").expect("s"),
    };
    assert!(round(&begin).contains(r#""trigger":"dictation_menu""#));
    round(&VoiceTarget::Shell);
    let app = VoiceTarget::App {
        app: AppName::parse("org.quire.Mail").expect("a"),
        serial: 7,
    };
    assert_eq!(
        round(&app),
        r#"{"kind":"app","v":{"app":"org.quire.Mail","serial":7}}"#
    );
    let speak = SpeakWire {
        utterance: Some(UtteranceId::parse("u-3").expect("u")),
        text: SpokenText("done".into()),
        class: DataClass::Mail,
        lang: LanguageTag::parse("en-US").expect("l"),
    };
    round(&speak);
    for e in [
        SpeechEnd::Done,
        SpeechEnd::Hushed,
        SpeechEnd::BargedIn,
        SpeechEnd::Failed(VoiceFault::NoModel),
    ] {
        round(&e);
    }
    round(&Envelope {
        vocab: VoiceVocab::CURRENT,
        body: begin,
    });
    assert_eq!(round(&VoiceVocab::CURRENT), "1");
}

#[test]
fn voice_refusal_maps_errors_1to1() {
    let mut names = std::collections::BTreeSet::new();
    for r in VoiceRefusal::ALL {
        let name = r.error_name();
        assert!(name.starts_with("org.quire.Voice1.Error."), "{name}");
        assert_eq!(VoiceRefusal::from_error_name(&name), Some(r));
        assert!(names.insert(name), "duplicate name for {r:?}");
        // The slug is the variant in snake_case.
        round(&r);
    }
    assert_eq!(names.len(), 6);
    assert_eq!(
        VoiceRefusal::from_error_name("org.quire.Voice1.Error.Nope"),
        None
    );
    // ALL is exhaustive: this match fails to compile when a variant is added.
    for r in VoiceRefusal::ALL {
        match r {
            VoiceRefusal::Disabled
            | VoiceRefusal::NeedsConsent
            | VoiceRefusal::Busy
            | VoiceRefusal::NotAllowed
            | VoiceRefusal::NoModel
            | VoiceRefusal::MicUnavailable => {}
        }
    }
}

#[test]
fn status_has_no_content() {
    let status = VoiceStatus {
        mic: MicState::Open {
            since: UnixSeconds(5),
            intent: VoiceIntent::Ask,
        },
        speaking: Speaking::Quiet,
        stt: Readiness::Ready,
        tts: Readiness::Loadable,
        enabled: VoiceUse::On,
    };
    let json = round(&status);
    let value: serde_json::Value = serde_json::from_str(&json).expect("value");
    let mut keys: Vec<&str> = value
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    // Another crate in a workspace build may turn on serde_json's key order: sort for a stable list.
    keys.sort_unstable();
    assert_eq!(keys, ["enabled", "mic", "speaking", "stt", "tts"]);
    for word in ["text", "audio", "pcm"] {
        assert!(!json.contains(word), "{json}");
    }
}

#[test]
fn transcripts_redact_debug() {
    let end = UtteranceEnd::Heard {
        text: HeardText("secret words".into()),
        served: served(),
    };
    assert!(!format!("{end:?}").contains("secret"));
    assert!(!format!("{:?}", SpokenText("secret".into())).contains("secret"));
}
