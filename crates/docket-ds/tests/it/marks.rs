//! Each pair of view mark and wire type converts totally and loses nothing it should keep.

use docket_core::*;
use docket_ds::*;
use ds_intents::{
    ChipKind, HeardEndMark, HeardMark, InputLevel, SummonAnswerMark, SummonOriginMark, Tally,
    ThingMark,
};
use porter_core::{AppName, Count};
use prov::{Integrity, Label, Labelled, Source, SpaceId};
use voice_wire::{
    CancelCause, HeardSegment, HeardTail, HeardText, Level, UtteranceEnd, VoiceEvent,
};

fn app() -> AppName {
    AppName::parse("org.quire.Mail").expect("app")
}
fn space() -> SpaceId {
    SpaceId::parse("work").expect("space")
}
fn label() -> Label {
    Label::trusted_user()
}
fn text(t: &str) -> Labelled<String> {
    Labelled {
        value: t.into(),
        label: label(),
    }
}

#[test]
fn summon_answers_convert_totally_both_ways() {
    let answers = [
        SummonAnswer::TookField,
        SummonAnswer::TookAnchored,
        SummonAnswer::Restored,
        SummonAnswer::Declined,
    ];
    let marks = [
        SummonAnswerMark::TookField,
        SummonAnswerMark::TookAnchored,
        SummonAnswerMark::Restored,
        SummonAnswerMark::Declined,
    ];
    for (answer, mark) in answers.into_iter().zip(marks) {
        assert_eq!(summon_answer_mark(answer), mark);
        assert_eq!(summon_answer_of(mark), answer);
    }
    assert_eq!(serial_of(serial_mark(SummonSerial(41))), SummonSerial(41));
}

fn snapshot(
    query: Option<&str>,
    results: u32,
    selection: Selection,
    privacy: WindowPrivacy,
) -> ContextSnapshot {
    ContextSnapshot {
        app: app(),
        window: text("Inbox"),
        here: match query {
            Some(q) => Here::View {
                view: ViewName::parse("search").expect("view"),
                query: Some(text(q)),
            },
            None => Here::Nowhere,
        },
        selection,
        visible: Visible {
            kind: None,
            items: vec![],
            total: Count(results),
        },
        text_target: TextTarget::None,
        privacy,
    }
}

#[test]
fn a_search_with_results_carries_a_query_and_a_results_chip() {
    let chips = chips_of(&snapshot(
        Some("lisbon receipts"),
        12,
        Selection::Nothing,
        WindowPrivacy::Normal,
    ));
    let kinds: Vec<ChipKind> = chips.iter().map(|c| c.kind).collect();
    assert_eq!(
        kinds,
        [
            ChipKind::Query,
            ChipKind::Results,
            ChipKind::Window,
            ChipKind::App
        ]
    );
    assert_eq!(chips[0].label, "lisbon receipts");
    assert_eq!(chips[1].count, Some(Tally(12)));
}

#[test]
fn a_private_window_shows_the_app_alone() {
    let chips = chips_of(&snapshot(
        Some("secret"),
        3,
        Selection::Nothing,
        WindowPrivacy::Private,
    ));
    assert_eq!(chips.len(), 1);
    assert_eq!(chips[0].kind, ChipKind::App);
    assert!(chips.iter().all(|c| !c.label.contains("secret")));
}

#[test]
fn removing_a_chip_drops_that_context_from_the_turn() {
    let mut chips = chips_of(&snapshot(
        Some("lisbon"),
        12,
        Selection::Nothing,
        WindowPrivacy::Normal,
    ));
    let all = keep_of(&chips);
    assert_eq!(
        (all.query, all.results, all.window),
        (Keep::Kept, Keep::Kept, Keep::Kept)
    );
    assert_eq!(
        all.selection,
        Keep::Dropped,
        "nothing was selected, so there is no chip to keep"
    );
    chips.retain(|c| c.kind != ChipKind::Results);
    let kept = keep_of(&chips);
    assert_eq!((kept.query, kept.results), (Keep::Kept, Keep::Dropped));
}

#[test]
fn a_thing_keeps_its_words_and_is_labelled_by_the_kinds_title_trust() {
    let mark = ThingMark {
        kind: "mail.thread".into(),
        key: "t1".into(),
        title: "Re: invoice".into(),
        subtitle: "Eve".into(),
    };
    let third = entity_ref(
        &app(),
        &space(),
        &mark,
        &TitleTrust::ThirdParty(Source::Mail),
    )
    .expect("a thing");
    assert_eq!(third.title.label.integrity, Integrity::Untrusted);
    assert!(third.title.label.sources.contains(&Source::Mail));
    assert_eq!(
        thing_mark(&third),
        mark,
        "the row's words survive the round trip"
    );
    let own = entity_ref(&app(), &space(), &mark, &TitleTrust::AppAuthored).expect("a thing");
    assert_eq!(own.title.label.integrity, Integrity::Trusted);
}

#[test]
fn a_mark_that_is_not_a_thing_is_refused() {
    let bad_kind = ThingMark {
        kind: "thread".into(),
        key: "t".into(),
        title: String::new(),
        subtitle: String::new(),
    };
    assert!(matches!(
        entity_ref(&app(), &space(), &bad_kind, &TitleTrust::AppAuthored),
        Err(ThingError::Kind(_))
    ));
    let bad_key = ThingMark {
        kind: "mail.thread".into(),
        key: String::new(),
        title: String::new(),
        subtitle: String::new(),
    };
    assert_eq!(
        entity_ref(&app(), &space(), &bad_key, &TitleTrust::AppAuthored),
        Err(ThingError::Key)
    );
}

#[test]
fn voice_frames_become_what_a_field_does() {
    let served = || porter_infer::ServedBy {
        account: porter_core::AccountId::parse("local").expect("account"),
        model: porter_core::ModelId::parse("nemotron").expect("model"),
        locality: porter_core::Locality::OnDevice,
    };
    let cases: Vec<(&str, VoiceEvent, Option<HeardMark>)> = vec![
        ("opened is not a field's business", VoiceEvent::Opened, None),
        (
            "waiting is not either",
            VoiceEvent::Waiting(porter_infer::Readiness::Loading),
            None,
        ),
        (
            "level",
            VoiceEvent::Level(Level(400)),
            Some(HeardMark::Level(InputLevel(400))),
        ),
        (
            "partial",
            VoiceEvent::Partial(HeardTail {
                text: HeardText("hel".into()),
            }),
            Some(HeardMark::Tail("hel".into())),
        ),
        (
            "committed",
            VoiceEvent::Committed(HeardSegment {
                text: HeardText("hello".into()),
            }),
            Some(HeardMark::Committed("hello".into())),
        ),
        (
            "heard end",
            VoiceEvent::Ended(UtteranceEnd::Heard {
                text: HeardText("hello there".into()),
                served: served(),
            }),
            Some(HeardMark::Ended(HeardEndMark::Send("hello there".into()))),
        ),
        (
            "nothing heard",
            VoiceEvent::Ended(UtteranceEnd::NothingHeard),
            Some(HeardMark::Ended(HeardEndMark::Nothing)),
        ),
        (
            "cancelled",
            VoiceEvent::Ended(UtteranceEnd::Cancelled(CancelCause::Escape)),
            Some(HeardMark::Ended(HeardEndMark::Cancelled)),
        ),
    ];
    for (name, event, want) in cases {
        assert_eq!(heard_of(&event), want, "case: {name}");
    }
}

#[test]
fn heard_debug_shows_no_words() {
    let shown = format!(
        "{:?} {:?}",
        HeardMark::Tail("my secret".into()),
        HeardMark::Ended(HeardEndMark::Send("my secret".into()))
    );
    assert!(!shown.contains("secret"), "{shown}");
}

#[test]
fn a_summon_origin_becomes_where_ds_says_it_came_from() {
    let utterance = UtteranceId::parse("u-1").expect("utterance");
    let voice = |intent| SummonOrigin::Voice {
        utterance: utterance.clone(),
        intent,
    };
    let cases = [
        (
            "double tap",
            SummonOrigin::DoubleTap,
            SummonOriginMark::Keyboard,
        ),
        (
            "a held voice prompt",
            voice(VoiceIntent::Ask),
            SummonOriginMark::Voice,
        ),
        (
            "a dictation",
            voice(VoiceIntent::Dictate),
            SummonOriginMark::Dictation,
        ),
    ];
    for (name, origin, want) in cases {
        assert_eq!(summon_origin_mark(&origin), want, "case: {name}");
    }
}

struct Bare;
impl ds_intents::ContextModel for Bare {
    fn thing(&self) -> Option<ThingMark> {
        None
    }
    fn things(&self) -> Vec<ThingMark> {
        vec![]
    }
}
impl WindowFacts for Bare {
    fn title(&self) -> String {
        "Inbox".into()
    }
    fn is_private(&self) -> WindowPrivacy {
        WindowPrivacy::Normal
    }
    fn space(&self) -> SpaceId {
        space()
    }
    fn titles_of(&self, _kind: &str) -> TitleTrust {
        TitleTrust::AppAuthored
    }
}

/// The mapping from the model is not built: the source reports the app alone, and never panics.
#[test]
fn an_unbuilt_context_source_reports_the_app_alone() {
    use docket_client::ContextSource;
    let snap = DsContextSource::new(app(), Bare, Bare).snapshot(ContextScope::ActiveWindow);
    assert_eq!(snap.app, app());
    assert_eq!(snap.privacy, WindowPrivacy::Private);
    assert_eq!(chips_of(&snap).len(), 1);
}
