//! What the companion hands the router to keep: records of its sessions, episodes and narratives
//! (the router merges a narrative into its own skeleton), what a session holds by handle, and the
//! narrowing of a subagent's policy from the person's words.

use crate::support::*;
use almanac_core::{
    Episode, EpisodeId, EpisodeKind, EpisodeOutcome, JsonText, Narrative, Skeleton,
};
use almanac_core::{Succession, UserText};
use docket_core::*;
use docket_fake::ScriptedWriter;
use prov::{AgentRef, Label, MessageKind, ReportStatus, SessionId, Source, UnixSeconds};

fn note(session: &SessionId, note: NoteAsk) -> IntentsRequest {
    IntentsRequest::SessionNote {
        session: session.clone(),
        note,
    }
}

fn episode_in(in_space: &str, task: &str) -> Episode {
    Episode {
        id: EpisodeId::parse(task).expect("episode"),
        agent: AgentRef::Companion,
        kind: EpisodeKind::Task,
        parent: None,
        space: space(in_space),
        started: UnixSeconds(0),
        ended: UnixSeconds(1),
        outcome: EpisodeOutcome::Done,
        skeleton: Skeleton {
            label: trusted(),
            asked: vec![],
            steps: vec![],
            touched: vec![],
            results: vec![],
        },
        narrative: None,
    }
}

fn episodes(router: &docket_router::Router<docket_fake::FakeSeams>) -> Vec<Episode> {
    router
        .seams
        .sink
        .records()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Episode(e) => Some(*e),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_record_of_the_companions_session_is_kept_for_the_sessions_own_space() {
    let router = router();
    let s = open(&router, "work", AgentRef::Companion).await;
    let record = SessionNote {
        slug: NoteSlug::parse("asked").expect("slug"),
        json: JsonText::parse(r#"{"kind":"closed"}"#).expect("json"),
    };
    let reply = ask(
        &router,
        &companion(),
        note(&s.session, NoteAsk::Record(record.clone())),
    )
    .await;
    assert_eq!(reply, IntentsReply::Done);
    let kept: Vec<_> = router
        .seams
        .sink
        .records()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Session {
                space, slug, json, ..
            } => Some((space, slug, json)),
            _ => None,
        })
        .collect();
    assert_eq!(kept, [(space("work"), record.slug, record.json)]);
    assert!(NoteSlug::parse("Not A Slug").is_err() && NoteSlug::parse("").is_err());
}

#[tokio::test]
async fn an_episode_is_kept_only_for_the_space_of_the_session_that_hands_it_over() {
    let router = router();
    let s = open(&router, "work", AgentRef::Companion).await;
    let elsewhere = ask(
        &router,
        &companion(),
        note(
            &s.session,
            NoteAsk::Episode(Box::new(episode_in("home", "t-8"))),
        ),
    )
    .await;
    assert_eq!(elsewhere, IntentsReply::Refused(WireRefusal::NotAllowed));
    let mut untrusted = episode_in("work", "t-8");
    untrusted.skeleton.label = mail_label("work");
    assert_eq!(
        ask(
            &router,
            &companion(),
            note(&s.session, NoteAsk::Episode(Box::new(untrusted)))
        )
        .await,
        IntentsReply::Refused(WireRefusal::NotAllowed),
        "a skeleton is the router's kind of words: trusted"
    );
    assert!(episodes(&router).is_empty());
    let kept = ask(
        &router,
        &companion(),
        note(
            &s.session,
            NoteAsk::Episode(Box::new(episode_in("work", "t-8"))),
        ),
    )
    .await;
    assert_eq!(kept, IntentsReply::Done);
    assert_eq!(episodes(&router).len(), 1);
}

#[tokio::test]
async fn a_narrative_names_its_episode_and_the_router_merges_it_into_its_own_skeleton() {
    let router = router();
    let s = open(&router, "work", AgentRef::Companion).await;
    say(&router, &s.session, "archive the digest").await;
    let narrative = Narrative {
        text: UserText::new("The person tidied the digest."),
        label: Label::untrusted(
            Source::Model(prov::ModelRole::Consolidator),
            prov::DataClass::Prompt,
            space("work"),
        ),
        by: prov::ModelRole::Consolidator,
    };
    let id = EpisodeId::parse(s.task.as_str()).expect("episode id");
    let early = ask(
        &router,
        &companion(),
        note(
            &s.session,
            NoteAsk::Narrative {
                episode: id.clone(),
                narrative: narrative.clone(),
            },
        ),
    )
    .await;
    assert_eq!(
        early,
        IntentsReply::Refused(WireRefusal::Malformed),
        "a task that has not ended has no skeleton to narrate"
    );

    ask(
        &router,
        &companion(),
        IntentsRequest::SessionClose {
            session: s.session.clone(),
        },
    )
    .await;
    let skeleton = episodes(&router).remove(0);
    assert!(skeleton.narrative.is_none());

    let narrated = ask(
        &router,
        &companion(),
        note(
            &s.session,
            NoteAsk::Narrative {
                episode: id,
                narrative: narrative.clone(),
            },
        ),
    )
    .await;
    assert_eq!(narrated, IntentsReply::Done);
    let all = episodes(&router);
    let second = all.last().expect("the narrated successor");
    assert_eq!(second.narrative.as_ref(), Some(&narrative));
    assert_eq!(
        second.narrates(&skeleton),
        Succession::Narrates,
        "the same episode, the same skeleton, now with its narrative"
    );
}

#[tokio::test]
async fn a_final_report_leaves_the_workers_episode_at_once_and_closing_leaves_no_second() {
    let router = router();
    let front = open(&router, "work", AgentRef::Companion).await;
    let worker = open(
        &router,
        "work",
        AgentRef::Worker {
            task: prov::TaskId::parse("t-90").expect("task"),
        },
    )
    .await;
    let report = IntentsRequest::MessageSend {
        session: worker.session.clone(),
        draft: MessageDraft {
            to: prov::Address::new(AgentRef::Companion, space("work")),
            thread: None,
            in_reply_to: None,
            kind: MessageKind::Report {
                status: ReportStatus::Done,
            },
            parts: vec![DraftPart::Text(prov::MessageText::new("2 steps taken"))],
        },
    };
    assert!(matches!(
        ask(&router, &companion(), report).await,
        IntentsReply::Delivered(_)
    ));
    let first = episodes(&router);
    assert_eq!(first.len(), 1, "{first:?}");
    assert_eq!(
        first[0].agent,
        AgentRef::Worker {
            task: worker.task.clone()
        }
    );
    assert_eq!(first[0].outcome, EpisodeOutcome::Done);
    ask(
        &router,
        &companion(),
        IntentsRequest::SessionClose {
            session: worker.session,
        },
    )
    .await;
    assert_eq!(episodes(&router).len(), 1, "the report already ended it");
    let _ = front;
}

#[tokio::test]
async fn a_session_says_what_it_holds_by_handle_in_shape_and_size_only() {
    let router = router();
    let s = open(&router, "work", AgentRef::Companion).await;
    let held = hold(
        &router,
        &s.session,
        "Your hotel receipt",
        mail_label("work"),
    );
    let IntentsReply::Handles(cards) = ask(
        &router,
        &companion(),
        IntentsRequest::SessionHandles {
            session: s.session.clone(),
        },
    )
    .await
    else {
        panic!("handles")
    };
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].handle, held);
    assert_eq!(cards[0].shape, HandleShape::Text);
    assert_eq!(
        cards[0].size,
        CharCount(18),
        "the router knows how long it is"
    );
    assert!(
        !serde_json::to_string(&cards)
            .expect("json")
            .contains("hotel"),
        "never the content"
    );
    let strange = ask(
        &router,
        &companion(),
        IntentsRequest::SessionHandles {
            session: SessionId::parse("s-404").expect("session"),
        },
    )
    .await;
    assert_eq!(strange, IntentsReply::Refused(WireRefusal::NoSuchSession));
}

fn read_only(task: &prov::TaskId) -> TaskPolicy {
    let mut policy = wide_policy(task, "work");
    policy.ceiling = prov::Effect::Read;
    policy.actions =
        std::collections::BTreeSet::from([ActionMatch::AppUpTo(mail_app(), prov::Effect::Read)]);
    policy
}

fn turn() -> UserTurn {
    UserTurn {
        id: TurnId(1),
        text: "only read it, send nothing".into(),
        at: UnixSeconds(5),
        from: TurnSource::Launcher,
        via: TurnVia::Typed,
    }
}

#[tokio::test]
async fn narrowing_a_subagent_never_widens_what_it_has() {
    let mut router = router();
    let s = open(&router, "work", AgentRef::Companion).await;
    give_policy(&router, &s.session, wide_policy(&s.task, "work"));
    router.seams.writer = ScriptedWriter::returning(Ok(read_only(&s.task)));
    let narrowed = ask(
        &router,
        &companion(),
        IntentsRequest::SessionNarrow {
            session: s.session.clone(),
            turn: turn(),
        },
    )
    .await;
    assert_eq!(narrowed, IntentsReply::Done);
    let held = |router: &docket_router::Router<docket_fake::FakeSeams>| {
        router
            .state
            .lock()
            .expect("lock")
            .sessions
            .get(&s.session)
            .and_then(|r| r.policy.clone())
            .expect("a policy")
    };
    let now = held(&router);
    assert_eq!(now.ceiling, prov::Effect::Read);
    assert_eq!(
        compare(&now, &wide_policy(&s.task, "work")),
        PolicyChange::Narrows
    );

    // A writer that now says "everything": the policy the session has is the bound.
    router.seams.writer = ScriptedWriter::returning(Ok(wide_policy(&s.task, "work")));
    ask(
        &router,
        &companion(),
        IntentsRequest::SessionNarrow {
            session: s.session.clone(),
            turn: turn(),
        },
    )
    .await;
    let after = held(&router);
    assert_eq!(
        after.ceiling,
        prov::Effect::Read,
        "a narrowing never widens"
    );
    assert!(matches!(
        compare(&after, &now),
        PolicyChange::Same | PolicyChange::Narrows
    ));

    // A writer that fails changes nothing.
    router.seams.writer = ScriptedWriter::failing();
    assert_eq!(
        ask(
            &router,
            &companion(),
            IntentsRequest::SessionNarrow {
                session: s.session.clone(),
                turn: turn(),
            },
        )
        .await,
        IntentsReply::Done
    );
    assert_eq!(held(&router), after);
}
