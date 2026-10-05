//! What happens when the world moves under a call: a halt that arrives while a reviewer or the
//! sheet is working, and a chain of follow-ups. These need seams that act mid-call, so the
//! rig below swaps the link, the reviewer and the confirmer for ones that do.

mod support;

use action_review::{ReviewVerdict, Reviewer};
use docket_core::*;
use docket_fake::{
    FakeMemory, FixedClock, MemoryGrants, RecordingSink, ScriptedReader, ScriptedWriter,
};
use docket_router::{AppLink, LinkFault, Router, Seams};
use policy_point::Pdp;
use prov::{AgentRef, SpaceScope, UnixSeconds};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use support::*;

type Late = Arc<OnceLock<Arc<Router<Rig>>>>;

/// Halts the Space the way the control centre would, as far as the router can tell.
fn halt(late: &Late) {
    if let Some(router) = late.get() {
        let mut st = router.state.lock().expect("lock");
        st.kill.spaces.insert(
            space("work"),
            docket_core::Halt::Halted {
                since: UnixSeconds(0),
                by: HaltCause::StopKey,
            },
        );
    }
}

struct HaltingReviewer(Late);
impl Reviewer for HaltingReviewer {
    async fn review(
        &self,
        _: Stage,
        _: &action_review::ReviewRequest,
    ) -> Result<ReviewVerdict, ReviewError> {
        halt(&self.0);
        Ok(ReviewVerdict::Allow)
    }
}

struct HaltingConfirmer {
    late: Late,
    cancelled: Mutex<Vec<ConfirmId>>,
}
impl Confirmer for HaltingConfirmer {
    async fn confirm(&self, request: ConfirmRequest) -> ConfirmAnswer {
        halt(&self.late);
        ConfirmAnswer::Allowed {
            scope: GrantScope::Once,
            receipt: prov::ConfirmReceipt {
                id: request.id,
                input: prov::InputProof::HardwareSeat,
                at: UnixSeconds(1),
                covers: prov::Confidentiality::Secret,
            },
        }
    }
    async fn cancel(&self, id: &ConfirmId) {
        self.cancelled.lock().expect("lock").push(id.clone());
    }
}

/// An app that answers every call with another call to make, and counts the calls it gets.
struct ChainLink {
    performed: AtomicU32,
}
impl AppLink for ChainLink {
    async fn perform(
        &self,
        _: &porter_core::AppName,
        _: Invocation,
        _: Latency,
    ) -> Result<Outcome, docket_router::AppFault> {
        self.performed.fetch_add(1, Ordering::SeqCst);
        Ok(Outcome {
            value: None,
            said: None,
            show: Preview::None,
            undo: Undoable::No,
            follow: Follow::Next(call("mail.thread.read", &["t1"], vec![])),
        })
    }
    async fn dry_run(
        &self,
        _: &porter_core::AppName,
        _: Invocation,
    ) -> Result<Preview, AppRefusal> {
        Ok(Preview::None)
    }
    async fn undo(
        &self,
        _: &porter_core::AppName,
        _: &UndoToken,
        _: &prov::Actor,
    ) -> Result<(), UndoFault> {
        Err(UndoFault::AppUnavailable)
    }
    async fn context(
        &self,
        _: &porter_core::AppName,
        _: ContextScope,
    ) -> Result<ContextSnapshot, LinkFault> {
        Err(LinkFault::Unavailable)
    }
    async fn search(
        &self,
        _: &porter_core::AppName,
        _: &str,
        _: Generation,
    ) -> Result<Vec<Hit>, LinkFault> {
        Err(LinkFault::Unavailable)
    }
    async fn preview(
        &self,
        _: &porter_core::AppName,
        _: &prov::EntityId,
    ) -> Result<Preview, LinkFault> {
        Err(LinkFault::Unavailable)
    }
    async fn suggest(
        &self,
        _: &porter_core::AppName,
        _: SuggestAsk,
    ) -> Result<Vec<EntityRef>, LinkFault> {
        Err(LinkFault::Unavailable)
    }
}

struct Rig {
    link: ChainLink,
    confirmer: HaltingConfirmer,
    reviewer: HaltingReviewer,
    grants: MemoryGrants,
    sink: RecordingSink,
    clock: FixedClock,
    memory: FakeMemory,
    writer: ScriptedWriter,
    reader: ScriptedReader,
}

impl Seams for Rig {
    type Link = ChainLink;
    type Confirm = HaltingConfirmer;
    type Review = HaltingReviewer;
    type Grants = MemoryGrants;
    type Sink = RecordingSink;
    type Time = FixedClock;
    type Memory = FakeMemory;
    type Writer = ScriptedWriter;
    type Reading = ScriptedReader;
    fn link(&self) -> &ChainLink {
        &self.link
    }
    fn confirmer(&self) -> &HaltingConfirmer {
        &self.confirmer
    }
    fn reviewer(&self) -> &HaltingReviewer {
        &self.reviewer
    }
    fn grants(&self) -> &MemoryGrants {
        &self.grants
    }
    fn sink(&self) -> &RecordingSink {
        &self.sink
    }
    fn clock(&self) -> &FixedClock {
        &self.clock
    }
    fn memory(&self) -> &FakeMemory {
        &self.memory
    }
    fn writer(&self) -> &ScriptedWriter {
        &self.writer
    }
    fn reader(&self) -> &ScriptedReader {
        &self.reader
    }
}

async fn rig(strictness: Strictness) -> (Arc<Router<Rig>>, SessionOpened) {
    let late: Late = Arc::new(OnceLock::new());
    let seams = Rig {
        link: ChainLink {
            performed: AtomicU32::new(0),
        },
        confirmer: HaltingConfirmer {
            late: late.clone(),
            cancelled: Mutex::new(vec![]),
        },
        reviewer: HaltingReviewer(late.clone()),
        grants: MemoryGrants::new(),
        sink: RecordingSink::new(),
        clock: FixedClock::at(UnixSeconds(0)),
        memory: FakeMemory::default(),
        writer: ScriptedWriter::failing(),
        reader: ScriptedReader::default(),
    };
    let config = AgentConfig {
        strictness,
        ..AgentConfig::default()
    };
    let router = Arc::new(Router::new(
        seams,
        config,
        Pdp::standard().expect("policies"),
    ));
    router.state.lock().expect("lock").registry = docket_fake::registry().expect("registry");
    late.set(router.clone()).ok().expect("set once");
    let opened = open_in(&router).await;
    (router, opened)
}

async fn open_in(router: &Router<Rig>) -> SessionOpened {
    let IntentsReply::SessionOpened(opened) = router
        .handle(
            &companion(),
            IntentsRequest::SessionOpen(SessionOpen {
                space: space("work"),
                agent: AgentRef::Companion,
                parent: None,
            }),
        )
        .await
    else {
        panic!("open")
    };
    {
        let mut st = router.state.lock().expect("lock");
        let record = st.sessions.get_mut(&opened.session).expect("session");
        record.policy = Some(wide_policy(&opened.task, "work"));
    }
    use docket_router::GrantStore;
    use porter_core::consent::{Decision, Grant, GrantScope as Scope, Usage};
    for (n, class) in [prov::DataClass::Mail, prov::DataClass::Contacts]
        .into_iter()
        .enumerate()
    {
        router.seams.grants.record(Grant {
            id: porter_core::GrantId::parse(&format!("g-{n}")).expect("grant"),
            key: ActionGrantKey {
                caller: GrantCaller::Companion,
                owner: mail_app(),
                target: GrantTarget::App,
                class,
                usage: Usage::Interactive,
                space: SpaceScope::Only(space("work")),
            },
            decision: Decision::Allow,
            scope: Scope::Always,
            at: UnixSeconds(0),
        });
    }
    opened
}

async fn run(router: &Router<Rig>, request: CallRequest) -> Result<Outcome, CallRefusal> {
    match router
        .handle(
            &companion(),
            IntentsRequest::Perform {
                activation: None,
                call: request,
                session: None,
                parent_window: None,
            },
        )
        .await
    {
        IntentsReply::Performed(result) => *result,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_halt_that_arrives_during_review_overrides_the_reviewers_allow() {
    // Judged calls review first: AskMore makes an archive reviewed.
    let (router, _s) = rig(Strictness::AskMore).await;
    let refused = run(&router, call("mail.thread.archive", &["t1"], vec![]))
        .await
        .expect_err("halted before it ran");
    assert_eq!(
        refused,
        CallRefusal::Halted(SpaceScope::Only(space("work")))
    );
    assert_eq!(
        router.seams.link.performed.load(Ordering::SeqCst),
        0,
        "the app was never called"
    );
}

#[tokio::test]
async fn a_halt_that_arrives_while_the_sheet_is_up_withdraws_it_and_stops_the_call() {
    let (router, _s) = rig(Strictness::Default).await;
    // An untrusted recipient asks; the sheet's answer is a yes, and a halt came first.
    let send = call(
        "mail.message.send",
        &[],
        vec![
            ("to", Value::Entity(entity("mail.contact", "eve@evil.test"))),
            ("body", Value::Text("hi".into())),
        ],
    );
    let refused = run(&router, send).await.expect_err("halted");
    assert_eq!(
        refused,
        CallRefusal::Halted(SpaceScope::Only(space("work")))
    );
    assert_eq!(router.seams.link.performed.load(Ordering::SeqCst), 0);
    let withdrawn = router
        .seams
        .confirmer
        .cancelled
        .lock()
        .expect("lock")
        .clone();
    assert_eq!(withdrawn.len(), 1, "the sheet was withdrawn");
    let granted = {
        use docket_router::GrantStore;
        router.seams.grants.grants().len()
    };
    assert_eq!(
        granted, 2,
        "a yes that came after the halt recorded no new grant"
    );
}

#[tokio::test]
async fn a_chain_of_follow_ups_stops_at_the_configured_depth() {
    let (router, _s) = rig(Strictness::Default).await;
    let outcome = run(&router, call("mail.thread.read", &["t1"], vec![]))
        .await
        .expect("the chain's last outcome");
    assert_eq!(
        outcome.follow,
        Follow::Nothing,
        "the refused follow is not offered on"
    );
    // Depth 0 and four follow-ups ran; the next was refused at the gate.
    assert_eq!(router.seams.link.performed.load(Ordering::SeqCst), 5);
    let ends: Vec<CallEnd> = router
        .seams
        .sink
        .records()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Call { end, .. } => Some(end),
            _ => None,
        })
        .collect();
    assert_eq!(ends.len(), 6);
    assert_eq!(
        ends[5],
        CallEnd::Refused(CallRefusal::OverBudget(BudgetKind::Chain))
    );
    assert!(ends[..5].iter().all(|e| *e == CallEnd::Done));
}
