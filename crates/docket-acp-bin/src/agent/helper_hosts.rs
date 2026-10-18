//! Where the hosts of one helper's agent processes come from on a live host: a new backend for
//! the helper's program each time its process starts, made for this helper (its grants are its
//! own, and so is the menu of actions its agent is offered), opening its session in a folder of the helper's own under docket's data directory.

use super::host::Hosting;
use super::live::Live;
use docket_acp::client::{AgentHost, EdgeMenu, Fallback, Hosts, Start};
use docket_core::helper::HelperCard;
use docket_core::{ProgramName, TaskPolicy};
use docket_inapp::EditorDesk;
use docket_launch::Accounts;
use docket_session::{BackendKind, Opening, Workspace};
use prov::{AgentRef, HelperId, SpaceId, TaskId};
use std::sync::Arc;

/// One helper's agent app, as a source of hosts.
pub struct HelperHosts<A: Accounts + 'static> {
    live: Arc<Live<A>>,
    helper: HelperId,
    program: ProgramName,
    space: SpaceId,
    task: TaskId,
    cwd: Option<Workspace>,
    /// What the person let the helper use: the menu of its agent's tool edge.
    reach: TaskPolicy,
}

impl<A: Accounts + 'static> std::fmt::Debug for HelperHosts<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HelperHosts")
            .field("helper", &self.helper)
            .field("program", &self.program)
            .finish_non_exhaustive()
    }
}

impl<A: Accounts + 'static> HelperHosts<A> {
    /// The hosts of `card`'s agent. The caller has checked that the card runs an app that is
    /// listed and that its folder can be made.
    pub fn new(live: Arc<Live<A>>, card: &HelperCard) -> Self {
        let program = live.program_or_stand_in(card.kind.program().map(ProgramName::from));
        let cwd = folder_of(&card.id).and_then(|text| Workspace::parse(&text).ok());
        Self {
            helper: card.id.clone(),
            space: card.space.clone(),
            task: live.task_of(&card.id),
            program,
            cwd,
            reach: card.reach.clone(),
            live,
        }
    }
}

/// The folder a helper's agent works in: its own, never the home folder or a project.
pub fn folder_of(helper: &HelperId) -> Option<String> {
    let name = super::serve::plain_name(helper.as_str())?;
    super::serve::folder_under("helpers", name).ok()
}

impl<A: Accounts + 'static> Hosts for HelperHosts<A> {
    type Seams = Hosting<A>;
    type Desk = EditorDesk;

    async fn host(&mut self, start: Start) -> AgentHost<Hosting<A>, EditorDesk> {
        let mut backend = self.live.backend(
            &self.program,
            EdgeMenu::Helper(Box::new(self.reach.clone())),
        );
        if let Some(way) = start.sign_in {
            backend = backend.with_sign_in(way);
        }
        if let Some(id) = &start.resume {
            backend = backend.resuming(id);
        }
        let rewind = self
            .live
            .entry(&self.program)
            .map(|entry| entry.rewind)
            .unwrap_or_default();
        AgentHost::new(
            backend,
            self.live.court.clone(),
            self.live.desk.clone(),
            Fallback::Off,
        )
        .with_rewind(rewind)
        .for_helper(self.helper.clone())
    }

    fn resume_args(&self) -> Option<Vec<String>> {
        self.live
            .entry(&self.program)
            .and_then(|entry| entry.resume.clone())
    }

    fn opening(&self) -> Opening {
        Opening {
            task: self.task.clone(),
            space: self.space.clone(),
            opener: None,
            agent: Some(AgentRef::Companion),
            backend: BackendKind::Acp(self.program.clone()),
            parent: None,
            forked_from: None,
            started_from: None,
            label: self
                .live
                .entry(&self.program)
                .and_then(|entry| entry.label.clone()),
            cwd: self.cwd.clone(),
            rewind: Default::default(),
        }
    }
}
