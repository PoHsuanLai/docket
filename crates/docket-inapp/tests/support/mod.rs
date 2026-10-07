//! Shared by the in-app tests: the sheet the test answers, a context that shows nothing, and the
//! scripted model transport of companiond's tests (one file, included by path, not copied).

#![allow(dead_code)]

pub mod host;
#[path = "../../../companiond/tests/support/infer.rs"]
pub mod infer;

use docket_client::ContextSource;
use docket_core::{
    ConfirmId, ConfirmRequest, ContextScope, ContextSnapshot, Here, Selection, TextTarget, Visible,
    WindowPrivacy,
};
use docket_inapp::{ConfirmSheet, SheetAnswer};
use porter_core::Count;
use prov::Labelled;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// The sheet the test answers from a queue, remembering what it was shown.
#[derive(Debug, Default, Clone)]
pub struct TestSheet {
    answers: Arc<Mutex<VecDeque<SheetAnswer>>>,
    shown: Arc<Mutex<Vec<ConfirmRequest>>>,
}

impl TestSheet {
    pub fn answering(answers: Vec<SheetAnswer>) -> Self {
        Self {
            answers: Arc::new(Mutex::new(answers.into())),
            shown: Arc::default(),
        }
    }
    pub fn shown(&self) -> Vec<ConfirmRequest> {
        self.shown.lock().expect("lock").clone()
    }
}

impl ConfirmSheet for TestSheet {
    async fn ask(&self, request: &ConfirmRequest) -> SheetAnswer {
        self.shown.lock().expect("lock").push(request.clone());
        self.answers
            .lock()
            .expect("lock")
            .pop_front()
            .unwrap_or(SheetAnswer::Dismissed)
    }
    async fn withdraw(&self, _id: &ConfirmId) {}
}

/// What the person is looking at: nothing in particular.
#[derive(Debug, Clone, Copy)]
pub struct Nowhere;

impl ContextSource for Nowhere {
    fn snapshot(&self, _scope: ContextScope) -> ContextSnapshot {
        ContextSnapshot {
            app: porter_core::AppName::parse("org.quire.Mail").expect("app"),
            window: Labelled {
                value: String::new(),
                label: prov::Label::trusted_user(),
            },
            here: Here::Nowhere,
            selection: Selection::Nothing,
            visible: Visible {
                kind: None,
                items: vec![],
                total: Count(0),
            },
            text_target: TextTarget::None,
            privacy: WindowPrivacy::Normal,
        }
    }
}

/// Standing consent: the companion may use Mail in the `work` Space, always (what the person gave
/// on an earlier day), so a policy-covered call is left to the reviewer and not asked on the sheet.
pub fn standing_mail_consent() -> docket_inapp::SessionGrants {
    use docket_core::{ActionGrantKey, GrantCaller, GrantTarget};
    use porter_core::consent::{Decision, Grant, GrantScope, Usage};
    use porter_core::{AppName, DataClass, GrantId};
    use prov::{SpaceId, SpaceScope, UnixSeconds};
    let grants = [DataClass::Mail, DataClass::Contacts]
        .into_iter()
        .enumerate()
        .flat_map(|(n, class)| {
            [Usage::Interactive, Usage::Background]
                .into_iter()
                .map(move |usage| (n, class, usage))
        })
        .enumerate()
        .map(|(id, (_, class, usage))| Grant {
            id: GrantId::parse(&format!("g-{id}")).expect("grant"),
            key: ActionGrantKey {
                caller: GrantCaller::Companion,
                owner: AppName::parse("org.quire.Mail").expect("app"),
                target: GrantTarget::App,
                class,
                usage,
                space: SpaceScope::Only(SpaceId::parse("work").expect("space")),
            },
            decision: Decision::Allow,
            scope: GrantScope::Always,
            at: UnixSeconds(0),
        })
        .collect();
    docket_inapp::SessionGrants::with(grants)
}
