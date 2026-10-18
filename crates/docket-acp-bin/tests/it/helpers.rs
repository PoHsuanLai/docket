//! The editor's helpers read from the daemon: a helper is listed only with a conversation to open,
//! its jobs come from its card, a sentence the board told reaches the sessions watching that
//! helper once the daemon says something changed, and an editor goes without helpers when the
//! daemon is not there. The daemon is a fake transport: no bus.

use companion_client::{Changes, HelpersTransport};
use companion_wire::{
    HelperCardView, HelperForm, HelperLine, HelperReply, HelpersView, JobLine, JobShown,
    RoutineCardView, RoutineForm, RoutineReply, RoutinesView, ToldWire,
};
use docket_acp::Helpers;
use docket_acp_bin::helpers::{BusHelpers, LiveHelpers};
use docket_client::TransportError;
use docket_core::helper::{HelperModel, HelperName, HelperPhase, JobId};
use docket_core::routine::RoutineId;
use futures_util::stream;
use porter_core::Count;
use prov::{EntityId, HelperId, SessionId, SpaceId};
use std::sync::atomic::{AtomicU64, Ordering};

fn helper(n: u64) -> HelperId {
    serde_json::from_str(&format!("\"h-{n}\"")).expect("helper id")
}

fn line(n: u64, name: &str, conversation: Option<&str>) -> HelperLine {
    HelperLine {
        id: helper(n),
        name: HelperName::parse(name).expect("name"),
        space: SpaceId::desktop(),
        doing: "Nothing to do".to_owned(),
        state: HelperPhase::Idle,
        needs_you: None,
        agent: None,
        conversation: conversation.map(|s| SessionId::parse(s).expect("session")),
    }
}

fn done_job() -> JobLine {
    JobLine {
        id: JobId(1),
        goal: "Compare the offers".to_owned(),
        state: JobShown::Done,
        note: None,
        answer: Some("The second is best.".to_owned()),
    }
}

/// A daemon with two helpers, one with a conversation. It tells one sentence from its second
/// read on, and says once that something changed.
struct Daemon {
    reads: AtomicU64,
}

impl Daemon {
    fn new() -> Self {
        Self {
            reads: AtomicU64::new(0),
        }
    }
}

fn nobody<T>() -> Result<T, TransportError> {
    Err(TransportError::Closed)
}

impl HelpersTransport for Daemon {
    async fn list(&self, after: u64) -> Result<HelpersView, TransportError> {
        let read = self.reads.fetch_add(1, Ordering::SeqCst);
        let told = match (read, after) {
            (0, _) | (_, 1..) => Vec::new(),
            _ => vec![ToldWire {
                n: 1,
                helper: helper(1),
                text: "Research finished 'Compare the offers'.".to_owned(),
            }],
        };
        Ok(HelpersView {
            helpers: vec![line(1, "Research", Some("s-61")), line(2, "Garden", None)],
            told,
        })
    }

    async fn card(&self, helper_id: &HelperId) -> Result<HelperCardView, TransportError> {
        let found = [line(1, "Research", Some("s-61")), line(2, "Garden", None)]
            .into_iter()
            .find(|l| &l.id == helper_id)
            .ok_or(TransportError::Closed)?;
        Ok(HelperCardView {
            jobs: if found.id == helper(1) {
                vec![done_job()]
            } else {
                Vec::new()
            },
            line: found,
            role: "Looks things up.".to_owned(),
            model: HelperModel::Unset,
            reach: Vec::new(),
            steps: Count(8),
            kind: Default::default(),
            login: Default::default(),
            sign_in: Vec::new(),
        })
    }

    async fn changes(&self) -> Result<Changes, TransportError> {
        Ok(Box::pin(stream::iter([()])))
    }

    async fn create(&self, _: &HelperForm) -> Result<HelperReply, TransportError> {
        nobody()
    }

    async fn edit(&self, _: &HelperId, _: &HelperForm) -> Result<HelperReply, TransportError> {
        nobody()
    }

    async fn pause(&self, _: &HelperId) -> Result<(), TransportError> {
        nobody()
    }

    async fn resume(&self, _: &HelperId) -> Result<(), TransportError> {
        nobody()
    }

    async fn remove(&self, _: &HelperId) -> Result<(), TransportError> {
        nobody()
    }

    async fn post_job(
        &self,
        _: &HelperId,
        _: &str,
        _: &[EntityId],
    ) -> Result<HelperReply, TransportError> {
        nobody()
    }

    async fn sign_in(&self, _: &HelperId, _: &str) -> Result<HelperReply, TransportError> {
        nobody()
    }

    async fn sign_out(&self, _: &HelperId) -> Result<HelperReply, TransportError> {
        nobody()
    }

    async fn routines(&self) -> Result<RoutinesView, TransportError> {
        nobody()
    }

    async fn routine_card(&self, _: RoutineId) -> Result<RoutineCardView, TransportError> {
        nobody()
    }

    async fn add_routine(&self, _: &RoutineForm) -> Result<RoutineReply, TransportError> {
        nobody()
    }

    async fn pause_routine(&self, _: RoutineId) -> Result<(), TransportError> {
        nobody()
    }

    async fn resume_routine(&self, _: RoutineId) -> Result<(), TransportError> {
        nobody()
    }

    async fn remove_routine(&self, _: RoutineId) -> Result<(), TransportError> {
        nobody()
    }
}

#[tokio::test]
async fn a_helper_is_listed_with_its_conversation_and_its_jobs_from_its_card() {
    let helpers = BusHelpers::connect(Daemon::new()).await.expect("connected");
    let rows = helpers.rows();
    assert_eq!(rows.len(), 1, "a helper with no conversation is not listed");
    assert_eq!(rows[0].name.as_str(), "Research");
    assert_eq!(rows[0].session.as_str(), "s-61");
    assert_eq!(helpers.jobs(&helper(1)), vec![done_job()]);
    assert!(helpers.jobs(&helper(2)).is_empty());
}

#[tokio::test]
async fn a_sentence_the_board_tells_arrives_once_the_daemon_says_something_changed() {
    let mut helpers = BusHelpers::connect(Daemon::new()).await.expect("connected");
    let told = helpers.next_told(&[helper(1)]).await;
    assert_eq!(told.text, "Research finished 'Compare the offers'.");
    // Nothing more is told: the daemon has no more to say.
    let more = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        helpers.next_told(&[helper(1)]),
    )
    .await;
    assert!(more.is_err(), "nothing more was told");
}

#[tokio::test]
async fn an_editor_goes_without_helpers_when_the_daemon_is_not_there() {
    let mut none = LiveHelpers::over(None).await;
    assert!(none.rows().is_empty());
    assert!(none.open_job(&helper(1)).await.is_err());
}

#[tokio::test]
async fn a_job_cannot_be_started_from_the_editor_and_the_refusal_is_in_plain_words() {
    let mut helpers = BusHelpers::connect(Daemon::new()).await.expect("connected");
    let refusal = helpers.open_job(&helper(1)).await.expect_err("refused");
    assert!(refusal.0.contains("Helpers window"), "{}", refusal.0);
}

#[tokio::test]
async fn a_session_is_in_a_helpers_voice_only_when_it_is_that_helpers_conversation() {
    use docket_acp_bin::voices::BusVoices;
    use docket_tasks::Voices;
    let voices = BusVoices::over(Daemon::new());
    let theirs = SessionId::parse("s-61").expect("session");
    let voice = voices.voice_of(&theirs).await.expect("the helper's voice");
    let role = voice.role.expect("a role");
    assert!(role.as_str().contains("You are Research"), "{role:?}");
    assert!(role.as_str().contains("Looks things up."), "{role:?}");
    let other = SessionId::parse("s-62").expect("session");
    assert_eq!(voices.voice_of(&other).await, None);
}
