//! The idle pass: the model-written account of a finished task, written at background priority
//! when the person has been away, from the trusted skeleton alone, behind its own label, and
//! given up the moment anything interactive starts.

mod support;

use agent_loop::IdlePhase;
use almanac_core::EpisodeKind;
use porter_core::consent::Usage;
use prov::{Confidentiality, Integrity, ModelRole, Source, UnixSeconds};
use serde_json::json;
use support::infer::{Say, call, words};
use support::world::*;

fn read_t1() -> Say {
    call(
        "org.quire.Mail-mail.thread.read",
        json!({ "target": { "app": "org.quire.Mail", "kind": "mail.thread", "key": "t1" } }),
    )
}

#[tokio::test]
async fn a_narrative_is_written_after_thirty_quiet_seconds_from_the_skeleton_alone() {
    let mut w = world(vec![
        read_t1(),
        words("Read it."),
        words("The person asked about the invoice and it was read."),
    ]);
    let opened = w.open("work").await;
    w.say(&opened.session, "read the invoice").await;
    assert_eq!(w.infer.asked().len(), 2);

    // Too soon: nothing is asked of the model.
    w.set_time(1_000 + 29);
    w.companion.tick().await.expect("tick");
    assert_eq!(w.infer.asked().len(), 2);
    assert_eq!(
        w.episodes()
            .iter()
            .filter(|e| e.narrative.is_some())
            .count(),
        0
    );

    w.set_time(1_000 + 31);
    w.companion.tick().await.expect("tick");
    let asked = w.infer.asked();
    assert_eq!(asked.len(), 3);
    let request = &asked[2];
    assert_eq!(request.usage, Usage::Background, "nobody is waiting for it");
    assert!(request.tools.is_empty(), "it can act on nothing");
    let seen = format!("{}{}", w.infer.system_text(2), w.infer.user_text(2));
    assert!(seen.contains("asked: read the invoice"), "{seen}");
    assert!(
        !seen.contains("IGNORE PREVIOUS"),
        "a task that read mail still gives the model only its skeleton: {seen}"
    );

    let narrated: Vec<_> = w
        .episodes()
        .into_iter()
        .filter(|e| e.narrative.is_some())
        .collect();
    assert_eq!(narrated.len(), 1, "{:?}", w.episodes());
    let narrative = narrated[0].narrative.as_ref().expect("narrative");
    assert_eq!(
        narrative.text.as_str(),
        "The person asked about the invoice and it was read."
    );
    assert_eq!(
        narrative.label.integrity,
        Integrity::Untrusted,
        "model words steer nothing"
    );
    assert!(
        narrative
            .label
            .sources
            .contains(&Source::Model(ModelRole::Consolidator))
    );
    assert_eq!(
        narrative.label.confidentiality,
        Confidentiality::Private([space("work")].into())
    );
    assert_eq!(narrated[0].kind, EpisodeKind::Task);
    assert_eq!(narrated[0].skeleton.label.integrity, Integrity::Trusted);
    assert!(w.companion.idle.queue.is_empty());

    // Written once.
    w.set_time(1_000 + 90);
    w.companion.tick().await.expect("tick");
    assert_eq!(w.infer.asked().len(), 3);
}

#[tokio::test]
async fn an_interactive_request_cancels_the_narrative_and_it_is_tried_again_later() {
    let mut w = world(vec![words("Done."), Say::Hang]);
    let first = w.open("work").await;
    w.say(&first.session, "hello").await;
    w.set_time(1_000 + 31);

    // The narrative is running (the model never answers) when the person speaks: the stream is
    // dropped, nothing is recorded, and the job goes back to the front of the queue.
    let shared = w.companion.shared.clone();
    let tick = w.companion.tick_at(UnixSeconds(1_000 + 31));
    let speaks = async {
        tokio::task::yield_now().await;
        shared.interrupt();
    };
    let (ticked, ()) = tokio::join!(tick, speaks);
    ticked.expect("tick");
    assert_eq!(w.infer.asked().len(), 2, "the narrative was asked for");
    assert!(w.episodes().iter().all(|e| e.narrative.is_none()));
    assert_eq!(w.companion.idle.queue.len(), 1);
    assert_eq!(w.companion.idle.phase, IdlePhase::Busy);

    // The person's turn runs as if nothing were going on, and the pass waits for them to go.
    w.infer
        .push(vec![words("Fine."), words("They said hello twice.")]);
    let second = w.open("work").await;
    w.say(&second.session, "hello again").await;
    assert_eq!(w.infer.asked().len(), 3);
    w.set_time(1_000 + 200);
    w.companion.tick().await.expect("tick");
    assert!(
        w.episodes().iter().any(|e| e.narrative.is_some()),
        "tried again once the person was away"
    );
}
