//! The runner: a harness over docket-fake's router, a maximal task policy, and the run itself.
//! The endings and the judgement of a finished case are `eval-core`'s.
//!
//! CI runs every corpus except `UiSpoofing` through the deterministic layers with a reviewer
//! that always allows (a fully hijacked judge) and a maximal task policy: the structural
//! guarantees must hold at 100%, because nothing a judge or a policy says can lift them.

use crate::steps::Player;
use crate::world::{install, state_of};
use docket_context::state::RouterState;
use docket_core::{AgentConfig, AuditRecord, ModelExchange};
use docket_fake::{
    FakeError, FakeLink, FakeSeams, Forget, MemoryGrants, RecordingSink, ScriptedConfirmer,
    ScriptedWriter, fake_router,
};
use docket_permissions_core::{ActionMatch, TaskPolicy, TaskPolicyState};
use docket_router::{Router, Seams};
use docket_vocab::LabelText;
use eval_core::{Case, CaseResult, CaseTrace, Cut, StepEnding};
use porter_ids::Count;
use prov::{Effect, SpaceId, TaskId, UnixSeconds};
use serde::{Deserialize, Serialize};

/// What a harness can run cases over: a router whose apps, sheet, consent store and event log
/// are the fakes (so a case can set its world up and read what happened), and whose reviewer can
/// forget between cases. The reviewer, the writer and the clock are free: the gate's are
/// scripted, a live run's are the real cascade and the real writer over inferd.
pub trait Rig:
    Seams<Link = FakeLink, Confirm = ScriptedConfirmer, Grants = MemoryGrants, Sink = RecordingSink>
{
    /// Drops what the reviewer recorded of the case before.
    fn forget_reviews(&self);
}

impl<S> Rig for S
where
    S: Seams<
            Link = FakeLink,
            Confirm = ScriptedConfirmer,
            Grants = MemoryGrants,
            Sink = RecordingSink,
        >,
    S::Review: Forget,
    S::Log: Forget,
{
    fn forget_reviews(&self) {
        self.reviewer().forget();
        self.log().forget();
    }
}

/// Where a case's task policy comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyMode {
    /// The widest policy a writer could produce is installed on the session (the gate's
    /// deterministic layers: nothing a writer says can matter).
    Maximal,
    /// The session starts with none: the router asks its writer, as it does for a person.
    Written,
}

/// A router over the fakes (by default with a hijacked judge) and the cases' world.
#[derive(Debug)]
pub struct Harness<S: Rig = FakeSeams> {
    /// The router, with every fake reachable through `router.seams`.
    pub router: Router<S>,
}

/// A policy that covers everything an app declares, up to destructive, in `space`: the widest
/// the writer could ever produce. It never covers an untrusted sink argument: `covers` refuses
/// those whatever the policy says.
pub fn maximal_policy(space: SpaceId, task: TaskId) -> TaskPolicy {
    let apps = [
        "org.quire.Mail",
        "org.quire.Files",
        "org.quire.Memory",
        "org.quire.Companion",
    ];
    TaskPolicy {
        task,
        space,
        from: vec![],
        actions: apps
            .iter()
            .filter_map(|a| porter_core::AppName::parse(a).ok())
            .map(|app| ActionMatch::AppUpTo(app, Effect::Destructive))
            .collect(),
        kinds: [
            "mail.thread",
            "mail.contact",
            "files.file",
            "memory.fact",
            "companion.task",
        ]
        .iter()
        .filter_map(|k| prov::EntityKind::parse(k).ok())
        .collect(),
        ceiling: Effect::Destructive,
        max_count: Count(u32::MAX),
        recipients: vec![],
        destinations: vec![],
        paths: vec![],
        expires: UnixSeconds(i64::MAX),
        rationale: LabelText::parse("maximal policy for the eval")
            .expect("a literal that obeys the label grammar"),
        state: TaskPolicyState::Active,
    }
}

impl Harness {
    /// A harness over `config`: the fake router, whose reviewer allows everything. `run_case`
    /// gives each case's session the maximal policy itself; [`Harness::maximal_writer`] is the
    /// same policy as a writer's result, for a test that wants the derivation path.
    pub fn new(config: AgentConfig) -> Result<Self, FakeError> {
        Ok(Self::over(fake_router(config)?))
    }

    /// The writer the harness installs: the maximal policy for `space`, as a scripted result.
    pub fn maximal_writer(space: SpaceId, task: TaskId) -> ScriptedWriter {
        ScriptedWriter::returning(Ok(maximal_policy(space, task)))
    }
}

impl<S: Rig> Harness<S> {
    /// A harness over a router built with whatever reviewer, writer and clock a run wants.
    pub fn over(router: Router<S>) -> Self {
        Self { router }
    }

    /// Forgets everything a case left behind: the apps' data and the link's log, the sheet, the reviewer's log, the
    /// event log, consent, and the router's sessions, tasks, journal and halts. Installed
    /// manifests stay.
    pub fn reset(&self) {
        let seams = &self.router.seams;
        seams.link().clear();
        seams.confirmer().clear();
        seams.forget_reviews();
        seams.sink().clear();
        seams.grants().clear();
        let mut st = state_of(&self.router);
        let registry = std::mem::take(&mut st.registry);
        *st = RouterState::new();
        st.registry = registry;
    }
}

/// A moment of a running case that a watcher is told about.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Moment {
    /// The world is built and the turns are recorded; no step has run.
    Installed,
    /// A step just ended.
    Stepped,
}

/// Runs one case: installs its world in the fakes, plays the person's turns and the scripted
/// steps through the router, and reports how each ended. A confirmation is answered with a
/// dismissal, so a call that asks never goes through.
pub fn run_case<S: Rig>(case: &Case, harness: &Harness<S>) -> CaseResult {
    play(case, harness, PolicyMode::Maximal, &mut |_| ())
}

/// [`run_case`] with a policy `mode` and a record of what happened: the person's turns, each
/// step as planned, every audit record and sheet it caused, and the model exchanges `drain`
/// hands over once the world is built and again after each step (a live run drains its tap, so
/// each exchange lands on the step that asked for it). The result is the one `run_case` gives.
pub fn run_case_traced<S: Rig>(
    case: &Case,
    harness: &Harness<S>,
    mode: PolicyMode,
    drain: &mut impl FnMut() -> Vec<ModelExchange>,
) -> (CaseResult, CaseTrace) {
    let seams = &harness.router.seams;
    let mut setup = Vec::new();
    let mut installed = 0;
    let mut cuts = Vec::new();
    let result = play(case, harness, mode, &mut |moment| match moment {
        Moment::Installed => {
            installed = seams.sink().records().len();
            setup = drain();
        }
        Moment::Stepped => cuts.push(Cut {
            records: seams.sink().records().len(),
            sheets: seams.confirmer().requests().len(),
            exchanges: drain(),
        }),
    });
    let records = seams.sink().records();
    let sheets = seams.confirmer().requests();
    let trace = CaseTrace::assemble(case, &result, (setup, installed), records, sheets, cuts);
    (result, trace)
}

fn play<S: Rig>(
    case: &Case,
    harness: &Harness<S>,
    mode: PolicyMode,
    watch: &mut impl FnMut(Moment),
) -> CaseResult {
    let router = &harness.router;
    let steps = match install(case, harness, mode) {
        Ok(scene) => {
            watch(Moment::Installed);
            Player::new(router, case, &scene).play(|_| watch(Moment::Stepped))
        }
        Err(fault) => {
            watch(Moment::Installed);
            watch(Moment::Stepped);
            vec![StepEnding::SetupFailed(fault.to_string())]
        }
    };
    let records = router.seams.sink().records();
    let tripped = records.iter().rev().find_map(|r| match r {
        AuditRecord::Breaker { trip, .. } => Some(*trip),
        _ => None,
    });
    let receipts = records
        .iter()
        .filter_map(|r| match r {
            AuditRecord::Confirm { input, .. } => *input,
            _ => None,
        })
        .collect();
    CaseResult {
        id: case.id.clone(),
        corpus: case.corpus,
        steps,
        tripped,
        receipts,
    }
}
