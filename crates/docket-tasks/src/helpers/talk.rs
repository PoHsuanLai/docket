//! The person's turn in a helper's conversation, run where the helper lives. The conversation is
//! a session the router opened with the helper's seat; the turn is an ordinary interactive one on
//! it, driven in the helper's voice (its role, and its model or pool asked for by the helper) and
//! offered only what its reach allows, exactly as one of its jobs is. Nothing about what the
//! helper may do widens: the router still gates every call under the seat.

use super::AsHelper;
use super::view::conversation_voice;
use crate::desk::SayFromPerson;
use crate::runtime::Companion;
use crate::seams::Surface;
use crate::tap::NoTap;
use companion_core::{JobEnding, JobReport, OpenedJob};
use companion_wire::AskWire;
use docket_client::Transport as IntentsTransport;
use docket_core::Clock;
use docket_core::helper::{HelperCard, JobFault};
use docket_core::{ContextKeep, Keep, SessionOpen, SessionOpened, TurnVia, UserTurn, WindowKey};
use porter_client::Transport as InferTransport;
use prov::AgentRef;

/// Where the window of a turn in a conversation is anchored: nowhere in particular.
const WINDOW: &str = "editor";

/// The chips of context such a turn keeps: none.
fn kept_nothing() -> ContextKeep {
    ContextKeep {
        query: Keep::Dropped,
        results: Keep::Dropped,
        selection: Keep::Dropped,
        window: Keep::Dropped,
    }
}

impl<P: InferTransport, I: IntentsTransport, K: Clock, S: Surface> Companion<P, I, K, S> {
    /// Runs the person's `said` in `card`'s conversation `talk` and reports how the turn ended.
    /// The conversation is taken up as a task of this companion the first time, and kept: the
    /// next turn sees the earlier ones. A report whose ending is `Done` carries only the words of
    /// this turn.
    pub async fn talk(
        &mut self,
        card: &HelperCard,
        talk: &OpenedJob,
        said: SayFromPerson,
    ) -> JobReport {
        if self.task_of(&talk.session).is_none() {
            let open = SessionOpen {
                space: card.space.clone(),
                agent: AgentRef::Companion,
                parent: None,
                cwd: None,
                started_from: None,
                external: None,
                helper: None,
                acts_for: None,
            };
            let opened = SessionOpened {
                session: talk.session.clone(),
                task: talk.task.clone(),
                mode: docket_core::Mode::default(),
            };
            self.adopt(&opened, &open);
        }
        self.refresh_catalogue().await;
        self.config.budget.calls = card.budget.steps;
        self.as_helper = Some(AsHelper {
            id: card.id.clone(),
            reach: card.reach.clone(),
        });
        let Ok(voice) = conversation_voice(&card.id, &card.name, card.role.as_str(), &card.model)
        else {
            return JobReport::failed(JobFault::Crashed);
        };
        self.planner.set_voice(voice);
        self.reopen(&talk.task);
        let before = self.runtimes.get(&talk.task).map_or(0, |rt| rt.said.len());
        let turn = UserTurn {
            id: said.id,
            text: said.text,
            at: self.clock.now(),
            from: said.from,
            via: TurnVia::Typed,
            mode: None,
        };
        let Ok(window) = WindowKey::parse(WINDOW) else {
            return JobReport::failed(JobFault::Crashed);
        };
        let asked = AskWire {
            session: talk.session.clone(),
            turn,
            keep: kept_nothing(),
            parent_window: window,
            app: None,
        };
        let Ok(begun) = self.begin_ask(asked) else {
            return JobReport::failed(JobFault::Crashed);
        };
        let ran = self.run_begun_tapped(begun, &mut NoTap).await;
        let mut report = self.job_report(&talk.task, ran);
        if let JobEnding::Done { words } = &mut report.ending
            && let Some(rt) = self.runtimes.get(&talk.task)
            && rt.said.len() > before
        {
            *words = rt.said[before..].join("\n");
        }
        report
    }
}
