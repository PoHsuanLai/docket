//! Routines and helper conversations on a private bus: a proposed routine waits for the shell to
//! accept or decline it, and the editor bridge speaks in a helper's conversation while a stranger
//! or another session is refused.

use crate::serve_helpers::EmptyRoutines;
use crate::support::bus::PrivateBus;
use crate::support::infer::{ScriptedInfer, words};
use crate::support::world::*;
use companion_wire::{
    HelperForm, HelperReply, HelpersView, RoutineCardView, RoutineForm, RoutineProposal,
    RoutineReply, RoutinesView, SayReply,
};
use companiond::serve_with_routines;
use docket_core::helper::{Ceiling, HelperModel, HelperName, Role};
use docket_core::routine::RoutineId;
use docket_core::*;
use docket_dbus::CompanionHelpers2Proxy;
use docket_helpers::RoutineDesk;
use docket_tasks::HelperDesk;
use docket_tasks::{Pending, RoutineAsk, RoutineFor};
use futures_util::lock::Mutex;
use prov::AgentRef;
use std::sync::Arc;

/// Routines that remember the forms they were asked to make.
#[derive(Debug, Default)]
struct RecordingRoutines {
    made: std::sync::Mutex<Vec<RoutineForm>>,
}

impl RoutineDesk for RecordingRoutines {
    fn view(&self) -> RoutinesView {
        RoutinesView::default()
    }

    fn card(&self, _routine: RoutineId) -> Option<RoutineCardView> {
        None
    }

    fn create(&self, form: RoutineForm) -> Pending<'_, RoutineReply> {
        self.made.lock().expect("made").push(form);
        Box::pin(async { RoutineReply::Made(RoutineId(1)) })
    }

    fn pause(&self, _routine: RoutineId) -> Pending<'_, ()> {
        Box::pin(async {})
    }

    fn resume(&self, _routine: RoutineId) -> Pending<'_, ()> {
        Box::pin(async {})
    }

    fn remove(&self, _routine: RoutineId) -> Pending<'_, ()> {
        Box::pin(async {})
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_proposed_routine_waits_for_the_shell_to_accept_or_decline_it() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let (server, shell, editor, stranger) = (
        bus.connect().await,
        bus.connect().await,
        bus.connect().await,
        bus.connect().await,
    );
    let mut w = world(vec![]);
    let hub = hub_of(&w);
    w.companion.set_desk(Arc::new(hub.clone()));
    let companion = Arc::new(Mutex::new(w.companion));
    let who = who(&server, scratch.path());
    let routines = Arc::new(RecordingRoutines::default());
    serve_with_routines(
        &server,
        companion,
        who.clone(),
        scratch.path().to_owned(),
        Some(routines.clone()),
    )
    .await
    .expect("serving");
    as_shell(&who, &shell);
    who.introduce_app(&editor, "org.quire.Acp", docket_dbus::callers::Role::Agent);

    let form = HelperForm {
        name: HelperName::parse("Research").expect("name"),
        role: Role::parse("Looks things up.").expect("role"),
        space: space("work"),
        model: HelperModel::Unset,
        apps: vec![app("org.quire.Mail")],
        ceiling: Ceiling::Look,
        steps: porter_core::Count(8),
        kind: Default::default(),
    };
    let helpers = CompanionHelpers2Proxy::new(&shell).await.expect("proxy");
    helpers
        .create(&serde_json::to_string(&form).expect("json"))
        .await
        .expect("create");
    let propose = |when: &str| RoutineAsk {
        helper: RoutineFor::Named("Research".to_owned()),
        goal: "Look up the news.".to_owned(),
        when: when.to_owned(),
        why: "You read the news every morning.".to_owned(),
        space: space("work"),
    };
    let proposals =
        |text: String| -> Vec<RoutineProposal> { serde_json::from_str(&text).expect("proposals") };

    // A time nobody can read, and a helper that is not there, are refused in words.
    assert!(matches!(
        hub.propose_routine(propose("whenever")).await,
        HelperReply::Refused(_)
    ));
    let nobody = RoutineAsk {
        helper: RoutineFor::Named("Nobody".to_owned()),
        ..propose("daily 08:00")
    };
    assert!(matches!(
        hub.propose_routine(nobody).await,
        HelperReply::Refused(_)
    ));
    assert!(proposals(helpers.routine_proposals().await.expect("none")).is_empty());

    // Two proposals are listed, and nothing is made before the shell answers.
    for when in ["daily 08:00", "every 2 hours"] {
        assert_eq!(hub.propose_routine(propose(when)).await, HelperReply::Done);
    }
    let listed = proposals(helpers.routine_proposals().await.expect("list"));
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].id.to_string(), "p-1");
    assert_eq!(listed[0].why, "You read the news every morning.");
    assert!(routines.made.lock().expect("made").is_empty());

    // The editor bridge may look and may not answer; a stranger may do neither.
    let theirs = CompanionHelpers2Proxy::new(&editor).await.expect("proxy");
    assert_eq!(
        proposals(theirs.routine_proposals().await.expect("read")).len(),
        2
    );
    assert!(theirs.accept_routine_proposal("p-1").await.is_err());
    assert!(theirs.decline_routine_proposal("p-1").await.is_err());
    let nobody = CompanionHelpers2Proxy::new(&stranger).await.expect("proxy");
    assert!(nobody.routine_proposals().await.is_err());
    assert!(nobody.accept_routine_proposal("p-1").await.is_err());
    assert_eq!(
        proposals(helpers.routine_proposals().await.expect("list")).len(),
        2
    );

    // Accepting makes the routine, skipping a missed time, and takes it off the list.
    let reply: RoutineReply = serde_json::from_str(
        &helpers
            .accept_routine_proposal("p-1")
            .await
            .expect("accept"),
    )
    .expect("reply");
    assert_eq!(reply, RoutineReply::Made(RoutineId(1)));
    let made = routines.made.lock().expect("made").clone();
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].missed, docket_core::routine::Missed::Skip);
    assert_eq!(made[0].goal, "Look up the news.");

    // Declining removes the other and makes nothing; a proposal already answered is not there.
    helpers
        .decline_routine_proposal("p-2")
        .await
        .expect("decline");
    assert!(proposals(helpers.routine_proposals().await.expect("list")).is_empty());
    assert_eq!(routines.made.lock().expect("made").len(), 1);
    let gone: RoutineReply = serde_json::from_str(
        &helpers
            .accept_routine_proposal("p-2")
            .await
            .expect("answered"),
    )
    .expect("reply");
    assert!(matches!(gone, RoutineReply::Refused(_)));
    assert!(helpers.accept_routine_proposal("nonsense").await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_editor_bridge_speaks_in_a_helpers_conversation_and_a_stranger_or_another_session_is_refused()
 {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let (server, shell, editor, stranger) = (
        bus.connect().await,
        bus.connect().await,
        bus.connect().await,
        bus.connect().await,
    );
    let mut w = world(vec![]);
    let research_model = ScriptedInfer::new(vec![words("Offer B is the cheapest.")]);
    let hub = hub_with(&w, &[("Research", &research_model)]);
    w.companion.set_desk(Arc::new(hub));
    let router = w.router.clone();
    let companion = Arc::new(Mutex::new(w.companion));
    let who = who(&server, scratch.path());
    serve_with_routines(
        &server,
        companion,
        who.clone(),
        scratch.path().to_owned(),
        Some(Arc::new(EmptyRoutines)),
    )
    .await
    .expect("serving");
    as_shell(&who, &shell);
    who.introduce_app(&editor, "org.quire.Acp", docket_dbus::callers::Role::Agent);

    let form = HelperForm {
        name: HelperName::parse("Research").expect("name"),
        role: Role::parse("Looks things up.").expect("role"),
        space: space("work"),
        model: HelperModel::Named(porter_infer::ModelRef {
            account: porter_core::AccountId::parse("local").expect("account"),
            model: porter_core::ModelId::parse("tiny").expect("model"),
        }),
        apps: vec![app("org.quire.Mail")],
        ceiling: Ceiling::Look,
        steps: porter_core::Count(8),
        kind: Default::default(),
    };
    let form = serde_json::to_string(&form).expect("json");
    let theirs = CompanionHelpers2Proxy::new(&shell).await.expect("proxy");
    let made: HelperReply =
        serde_json::from_str(&theirs.create(&form).await.expect("create")).expect("reply");
    let HelperReply::Made(_) = made else {
        panic!("a helper is made: {made:?}")
    };
    let listed: HelpersView =
        serde_json::from_str(&theirs.list(0).await.expect("list")).expect("view");
    let talk = listed.helpers[0]
        .conversation
        .clone()
        .expect("conversation");

    // The editor records the person's words with the router, as its own surface.
    let recorder = docket_client::Intents::over(docket_client::InProcess::new(
        router.clone(),
        caller("org.quire.Acp", CallerRole::Editor),
    ));
    let turn = recorder
        .session_turn(
            talk.clone(),
            TurnIn {
                text: "which offer is the cheapest?".into(),
                origin: Origin::InWindowField,
                keep: keep_nothing(),
                via: TurnVia::Typed,
                mode: None,
            },
        )
        .await
        .expect("turn");

    // The bridge speaks in the conversation, and the helper's own model answers.
    let helpers = CompanionHelpers2Proxy::new(&editor).await.expect("proxy");
    let said: SayReply = serde_json::from_str(
        &helpers
            .say(talk.as_str(), turn.0, "which offer is the cheapest?")
            .await
            .expect("say"),
    )
    .expect("reply");
    assert_eq!(said, SayReply::Said("Offer B is the cheapest.".to_owned()));
    let asked = research_model.opened();
    assert_eq!(
        asked.len(),
        1,
        "the helper's model, and no other, was asked"
    );

    // A session that is not a helper's conversation is refused in words, and no model is asked.
    let other = router_session(&router, "work").await;
    let refused: SayReply =
        serde_json::from_str(&helpers.say(other.as_str(), 1, "hello").await.expect("say"))
            .expect("reply");
    assert!(matches!(refused, SayReply::Refused(_)), "{refused:?}");
    assert!(helpers.say("not a session", 1, "hello").await.is_err());
    assert_eq!(research_model.opened().len(), 1);

    // A process nothing names speaks nowhere.
    let nobody = CompanionHelpers2Proxy::new(&stranger).await.expect("proxy");
    assert!(nobody.say(talk.as_str(), turn.0, "hello").await.is_err());
    assert_eq!(research_model.opened().len(), 1);
}

/// A session the companion's identity opens at the router, which is no helper's.
async fn router_session(
    router: &std::sync::Arc<docket_router::Router<docket_fake::FakeSeams>>,
    in_space: &str,
) -> prov::SessionId {
    let companion = docket_client::Intents::over(docket_client::InProcess::new(
        router.clone(),
        caller("org.quire.Companiond", CallerRole::Companion),
    ));
    companion
        .session_open(SessionOpen {
            space: space(in_space),
            agent: AgentRef::Companion,
            parent: None,
            cwd: None,
            started_from: None,
            external: None,
            helper: None,
            acts_for: None,
        })
        .await
        .expect("open")
        .session
}
