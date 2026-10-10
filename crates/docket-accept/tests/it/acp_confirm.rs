//! The bus half of an editor's sheets, without the rest of the world: intentd's `SheetConfirmer`
//! asks the `Confirm1` the docket-acp process serves (`ConfirmObject` over an `EditorDesk`), the
//! host's side of the desk hands the sheet out and answers it, and what comes back is the
//! router's answer. The roles are the shipped `intentd.toml`'s, read through the ACP gate.

use docket_accept::confirm::Verdict;
use docket_acp_bin::confirm::ConfirmObject;
use docket_core::{
    Anchor, ConfirmAnswer, ConfirmDetail, ConfirmEnd, ConfirmId, ConfirmOffer, ConfirmParts,
    ConfirmRequest, Confirmer, EditorRoute, Gesture, GrantScope, LabelText, Seconds, TaintNote,
};
use docket_dbus::{CONFIRM_PATH, ConfirmProxy, INTENTS_BUS};
use docket_inapp::EditorDesk;
use docket_session::{SheetChoice, SheetDesk};
use docket_settings::AcpExpose;
use docket_testbus::PrivateBus;
use intentd::{AcpGate, IntentdConfig, ProcRoot, SheetConfirmer};
use porter_core::{AppName, Count};
use prov::{Actor, ClientName, Effect, SessionId, SpaceId};
use std::sync::Arc;

const ACP: &str = "org.quire.Acp";

/// Everything on a private bus: "intentd" (owns `Intents1`, asks), the acp process (owns its name
/// and serves `Confirm1` over `desk`), and a scripted sill that would answer if it were asked.
struct Fixture {
    _dir: tempfile::TempDir,
    _bus: PrivateBus,
    confirmer: SheetConfirmer,
    desk: EditorDesk,
    acp: Option<docket_dbus::BusConnection>,
    sill: docket_accept::confirm::Sheet,
    stranger: docket_dbus::BusConnection,
    _keep: Vec<docket_dbus::BusConnection>,
}

impl Fixture {
    async fn start(gate: AcpGate) -> Fixture {
        let dir = tempfile::tempdir().expect("scratch");
        let bus = PrivateBus::start(dir.path());
        let intentd = bus.connect().await;
        intentd.request_name(INTENTS_BUS).await.expect("name");
        let acp = bus.connect().await;
        let desk = EditorDesk::new();
        acp.object_server()
            .at(CONFIRM_PATH, ConfirmObject::new(desk.clone(), FixedClock))
            .await
            .expect("served");
        acp.request_name(ACP).await.expect("name");
        let sill_connection = bus.connect().await;
        let (sill, _requests) = docket_accept::confirm::serve(&sill_connection)
            .await
            .expect("sill");
        sill.will(Verdict::Allow);
        let config = Arc::new(IntentdConfig::shipped().expect("shipped"));
        let proc = ProcRoot::Fake(dir.path().join("proc"));
        let confirmer = SheetConfirmer::trusting(intentd.clone(), config).gated(gate, &proc);
        let stranger = bus.connect().await;
        Fixture {
            _dir: dir,
            _bus: bus,
            confirmer,
            desk,
            acp: Some(acp),
            sill,
            stranger,
            _keep: vec![intentd, sill_connection],
        }
    }

    async fn acp_leaves(&mut self) {
        let gone = self.acp.take().expect("still there");
        drop(gone);
        // The bus notices the connection closing before the next owner lookup answers.
        let probe = self.stranger.clone();
        let dbus = zbus::fdo::DBusProxy::new(&probe).await.expect("proxy");
        let name = zbus::names::BusName::try_from(ACP).expect("name");
        while dbus.name_has_owner(name.clone()).await.unwrap_or(false) {
            tokio::task::yield_now().await;
        }
    }

    fn sill_was_asked(&self) -> usize {
        self.sill.shown().len()
    }

    /// Whether a connection that is not intentd is refused when it asks the acp process.
    async fn stranger_asks(&self, ask: &ConfirmRequest) -> bool {
        let text = serde_json::to_string(ask).expect("json");
        let proxy = ConfirmProxy::builder(&self.stranger)
            .destination(ACP)
            .expect("destination")
            .build()
            .await
            .expect("proxy");
        proxy.confirm(&text).await.is_err()
    }
}

/// The receipts' clock: any instant will do.
#[derive(Debug, Clone, Copy)]
struct FixedClock;

impl docket_router::Clock for FixedClock {
    fn now(&self) -> prov::UnixSeconds {
        prov::UnixSeconds(1)
    }

    fn after(&self, _wait: docket_core::Millis) -> impl std::future::Future<Output = ()> + Send {
        std::future::pending()
    }
}

fn route(session: &SessionId) -> EditorRoute {
    EditorRoute {
        client: ClientName::parse(ACP).expect("client"),
        session: session.clone(),
    }
}

fn request(id: &str, offer: ConfirmOffer) -> ConfirmRequest {
    ConfirmRequest::new(ConfirmParts {
        id: ConfirmId::parse(id).expect("id"),
        space: SpaceId::desktop(),
        actor: Actor::Unknown,
        app: AppName::parse("org.quire.Mail").expect("app"),
        action: LabelText::parse("Send a message").expect("label"),
        effect: Effect::Outbound,
        count: Count(1),
        detail: ConfirmDetail::Plain,
        lines: Vec::new(),
        why: Vec::new(),
        taint: TaintNote::Clean,
        offer,
        gesture: Gesture::Press,
        anchor: Anchor::Centre,
        expires: Seconds(120),
    })
}

fn session() -> SessionId {
    SessionId::parse("s-1").expect("session")
}

fn scope_of(answer: &ConfirmAnswer) -> Option<GrantScope> {
    match answer {
        ConfirmAnswer::Allowed { scope, .. } => Some(scope.clone()),
        _ => None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sheet_for_an_editor_session_is_shown_on_the_acp_desk_and_answered_back() {
    let fixture = Fixture::start(AcpGate::new(AcpExpose::On)).await;
    let ask = request("c-1", ConfirmOffer::OnceOrAlways).for_editor(Some(route(&session())));
    for (choice, scope) in [
        (SheetChoice::Once, Some(GrantScope::Once)),
        (SheetChoice::Always, Some(GrantScope::Always)),
        (SheetChoice::Refused, None),
    ] {
        let confirmer = fixture.confirmer.clone();
        let sent = ask.clone();
        let asking = tokio::spawn(async move { confirmer.confirm(sent).await });
        let shown = fixture
            .desk
            .next_sheet(&session())
            .await
            .expect("the sheet reaches the desk");
        assert_eq!(shown.id, ask.id);
        fixture.desk.answer(&shown.id, choice).expect("answered");
        let answer = asking.await.expect("the ask ends");
        assert_eq!(scope_of(&answer), scope, "{choice:?}: {answer:?}");
        if scope.is_none() {
            assert_eq!(answer, ConfirmAnswer::Ended(ConfirmEnd::Refused));
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_the_setting_off_the_acp_process_is_not_trusted_and_the_sheet_expires() {
    let fixture = Fixture::start(AcpGate::shut()).await;
    let ask = request("c-2", ConfirmOffer::OnceOnly).for_editor(Some(route(&session())));
    let answer = fixture.confirmer.confirm(ask).await;
    assert_eq!(answer, ConfirmAnswer::Ended(ConfirmEnd::Expired));
    assert_eq!(fixture.desk.waiting(), 0, "nothing reached the desk");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_the_acp_process_gone_the_sheet_expires_and_is_not_moved_to_the_desktop() {
    let mut fixture = Fixture::start(AcpGate::new(AcpExpose::On)).await;
    fixture.acp_leaves().await;
    let ask = request("c-3", ConfirmOffer::OnceOnly).for_editor(Some(route(&session())));
    let answer = fixture.confirmer.confirm(ask).await;
    assert_eq!(answer, ConfirmAnswer::Ended(ConfirmEnd::Expired));
    // A sill that would say yes is on the bus, and was not asked.
    assert_eq!(fixture.sill_was_asked(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_intentd_may_put_a_sheet_on_the_acp_desk() {
    let fixture = Fixture::start(AcpGate::new(AcpExpose::On)).await;
    let ask = request("c-4", ConfirmOffer::OnceOnly).for_editor(Some(route(&session())));
    let refused = fixture.stranger_asks(&ask).await;
    assert!(refused, "a connection that is not intentd is refused");
    assert_eq!(fixture.desk.waiting(), 0);
}
