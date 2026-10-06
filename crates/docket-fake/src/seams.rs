//! `FakeLink` over the two fake apps, and `FakeSeams` bundling every fake into the router's
//! `Seams`.

use crate::clock::FixedClock;
use crate::files::FakeFiles;
use crate::mail::FakeMail;
use crate::scripted::{
    FakeMemory, ScriptedConfirmer, ScriptedReader, ScriptedReviewer, ScriptedWriter,
};
use crate::simple::{MemoryGrants, RecordingSink};
use action_review::Reviewer;
use docket_client::IntentProvider;
use docket_core::PolicyWriter;
use docket_core::{
    AppRefusal, ContextScope, ContextSnapshot, EntityRef, Generation, Hit, Invocation, Latency,
    Outcome, Preview, SuggestAsk, UndoFault, UndoToken,
};
use docket_router::{AppFault, AppLink, Clock, LinkFault, Seams};
use porter_core::AppName;
use prov::{Actor, EntityId};
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

/// Whether a fake app answers `Perform`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answering {
    /// It answers.
    Normal,
    /// It never answers: the router times the call out.
    Silent,
    /// It is not there (not installed, or it does not start): the call is refused as unavailable.
    Absent,
}

/// Routes the router's calls to the fake apps by bus name.
#[derive(Debug)]
pub struct FakeLink {
    /// The mail app.
    pub mail: FakeMail,
    /// The files app.
    pub files: FakeFiles,
    /// The menu app, whose `menu.item.activate` classifies per call.
    pub menu: crate::menu::FakeMenu,
    /// Apps that do not answer `Perform` as usual: they time out, or are not there.
    pub answering: Mutex<BTreeMap<AppName, Answering>>,
    /// What the focused window shows, if a test set one: the fakes have no windows of their own.
    pub window: Mutex<Option<ContextSnapshot>>,
    /// Every invocation the router sent to `Perform`, in order.
    pub performed: Mutex<Vec<docket_core::ActivatedInvocation>>,
    /// The built-in `org.quire.Companion` provider, once a test has attached the router that
    /// hosts it (`host_companion`); until then the app is unavailable.
    companion: Hosted,
}

type Perform = Box<dyn Fn(Invocation) -> Result<Outcome, AppRefusal> + Send + Sync>;

/// The way back to the router that hosts the built-in provider.
#[derive(Default)]
struct Hosted(OnceLock<Perform>);

impl std::fmt::Debug for Hosted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Hosted(attached: {})", self.0.get().is_some())
    }
}

/// Attaches `router` as the host of the built-in `org.quire.Companion` provider, as intentd does:
/// the app answers through the router's own `companion_perform`. The router is held weakly.
pub fn host_companion(router: &std::sync::Arc<docket_router::Router<FakeSeams>>) {
    let weak = std::sync::Arc::downgrade(router);
    let _ = router
        .seams
        .link
        .companion
        .0
        .set(Box::new(move |inv| match weak.upgrade() {
            Some(router) => router.companion_perform(inv),
            None => Err(AppRefusal::Unsupported),
        }));
}

impl FakeLink {
    /// The three apps, answering.
    pub fn new(mail: FakeMail, files: FakeFiles, menu: crate::menu::FakeMenu) -> Self {
        Self {
            mail,
            files,
            menu,
            answering: Mutex::new(BTreeMap::new()),
            window: Mutex::new(None),
            performed: Mutex::new(Vec::new()),
            companion: Hosted::default(),
        }
    }

    /// Makes `snapshot` what the app's `Context` answers (until the next call to this).
    pub fn show_window(&self, snapshot: ContextSnapshot) {
        if let Ok(mut window) = self.window.lock() {
            *window = Some(snapshot);
        }
    }

    /// Makes `app` stop answering `Perform` (its calls time out), vanish (its calls are
    /// unavailable), or answer again.
    pub fn answer_from(&self, app: &AppName, answering: Answering) {
        if let Ok(mut all) = self.answering.lock() {
            match answering {
                Answering::Normal => all.remove(app),
                other => all.insert(app.clone(), other),
            };
        }
    }

    fn is_mail(&self, app: &AppName) -> bool {
        self.mail.manifest().manifest().app == *app
    }

    fn is_menu(&self, app: &AppName) -> bool {
        self.menu.manifest().manifest().app == *app
    }

    fn is_files(&self, app: &AppName) -> bool {
        self.files.manifest().manifest().app == *app
    }
}

impl AppLink for FakeLink {
    async fn perform(
        &self,
        app: &AppName,
        inv: Invocation,
        within: Latency,
    ) -> Result<Outcome, AppFault> {
        self.perform_activated(app, inv, None, within).await
    }

    async fn perform_activated(
        &self,
        app: &AppName,
        inv: Invocation,
        activation: Option<docket_core::ActivationToken>,
        within: Latency,
    ) -> Result<Outcome, AppFault> {
        self.perform_classified(app, inv, activation, None, within)
            .await
    }

    async fn classify(
        &self,
        app: &AppName,
        inv: Invocation,
    ) -> Result<docket_core::CallClass, docket_core::ClassifyFault> {
        if !self.is_menu(app) {
            return Err(docket_core::ClassifyFault::Unsupported);
        }
        self.menu
            .classify(inv)
            .await
            .map_err(|_| docket_core::ClassifyFault::Refused)
    }

    async fn perform_classified(
        &self,
        app: &AppName,
        inv: Invocation,
        activation: Option<docket_core::ActivationToken>,
        classified: Option<docket_core::CallClass>,
        _within: Latency,
    ) -> Result<Outcome, AppFault> {
        if let Ok(mut seen) = self.performed.lock() {
            seen.push(docket_core::ActivatedInvocation {
                invocation: inv.clone(),
                activation,
                classified: classified.clone(),
            });
        }
        let how = self
            .answering
            .lock()
            .ok()
            .and_then(|all| all.get(app).copied());
        match how {
            Some(Answering::Silent) => return Err(AppFault::TimedOut),
            Some(Answering::Absent) => return Err(AppFault::Unavailable),
            Some(Answering::Normal) | None => {}
        }
        if self.is_mail(app) {
            self.mail.perform(inv).await.map_err(AppFault::Refused)
        } else if self.is_files(app) {
            self.files.perform(inv).await.map_err(AppFault::Refused)
        } else if self.is_menu(app) {
            self.menu
                .perform_classified(inv, None, classified)
                .await
                .map_err(AppFault::Refused)
        } else if app.as_str() == docket_router::COMPANION_APP {
            match self.companion.0.get() {
                Some(perform) => perform(inv).map_err(AppFault::Refused),
                None => Err(AppFault::Unavailable),
            }
        } else {
            Err(AppFault::Refused(AppRefusal::Unsupported))
        }
    }

    async fn dry_run(&self, app: &AppName, inv: Invocation) -> Result<Preview, AppRefusal> {
        if self.is_mail(app) {
            self.mail.dry_run(inv).await
        } else if self.is_files(app) {
            self.files.dry_run(inv).await
        } else {
            Err(AppRefusal::Unsupported)
        }
    }

    async fn undo(&self, app: &AppName, token: &UndoToken, actor: &Actor) -> Result<(), UndoFault> {
        if self.is_mail(app) {
            self.mail.undo(token.clone(), actor.clone()).await
        } else if self.is_files(app) {
            self.files.undo(token.clone(), actor.clone()).await
        } else {
            Err(UndoFault::AppUnavailable)
        }
    }

    async fn context(
        &self,
        app: &AppName,
        _scope: ContextScope,
    ) -> Result<ContextSnapshot, LinkFault> {
        // The fakes have no windows of their own (ds answers context in a real app): a test
        // sets one with `show_window`.
        self.window
            .lock()
            .ok()
            .and_then(|window| window.clone())
            .filter(|snapshot| snapshot.app == *app)
            .ok_or(LinkFault::Unavailable)
    }

    async fn search(
        &self,
        app: &AppName,
        text: &str,
        _generation: Generation,
    ) -> Result<Vec<Hit>, LinkFault> {
        if self.is_mail(app) {
            Ok(self.mail.search(text).await)
        } else if self.is_files(app) {
            Ok(self.files.search(text).await)
        } else {
            Err(LinkFault::Unavailable)
        }
    }

    async fn preview(&self, app: &AppName, id: &EntityId) -> Result<Preview, LinkFault> {
        if self.is_mail(app) {
            Ok(self.mail.preview(id).await)
        } else if self.is_files(app) {
            Ok(self.files.preview(id).await)
        } else {
            Err(LinkFault::Unavailable)
        }
    }

    async fn suggest(&self, app: &AppName, ask: SuggestAsk) -> Result<Vec<EntityRef>, LinkFault> {
        if self.is_mail(app) {
            Ok(self.mail.suggest(ask).await)
        } else if self.is_files(app) {
            Ok(self.files.suggest(ask).await)
        } else {
            Err(LinkFault::Unavailable)
        }
    }
}

/// Every fake, public so a test reads what each recorded. The reviewer, the writer and the clock
/// are parameters whose defaults are the fakes: a live run puts the real cascade, the real
/// writer and the system clock in their places and keeps every other fake.
#[derive(Debug)]
pub struct FakeSeams<R = ScriptedReviewer, W = ScriptedWriter, K = FixedClock> {
    /// The apps.
    pub link: FakeLink,
    /// The sheet.
    pub confirmer: ScriptedConfirmer,
    /// The reviewer.
    pub reviewer: R,
    /// The consent store.
    pub grants: MemoryGrants,
    /// The event log.
    pub sink: RecordingSink,
    /// The clock.
    pub clock: K,
    /// Memory.
    pub memory: FakeMemory,
    /// The policy writer.
    pub writer: W,
    /// The reader.
    pub reader: ScriptedReader,
}

impl<R: Reviewer, W: PolicyWriter, K: Clock> Seams for FakeSeams<R, W, K> {
    type Link = FakeLink;
    type Confirm = ScriptedConfirmer;
    type Review = R;
    type Grants = MemoryGrants;
    type Sink = RecordingSink;
    type Time = K;
    type Memory = FakeMemory;
    type Writer = W;
    type Reading = ScriptedReader;

    fn link(&self) -> &FakeLink {
        &self.link
    }
    fn confirmer(&self) -> &ScriptedConfirmer {
        &self.confirmer
    }
    fn reviewer(&self) -> &R {
        &self.reviewer
    }
    fn grants(&self) -> &MemoryGrants {
        &self.grants
    }
    fn sink(&self) -> &RecordingSink {
        &self.sink
    }
    fn clock(&self) -> &K {
        &self.clock
    }
    fn memory(&self) -> &FakeMemory {
        &self.memory
    }
    fn writer(&self) -> &W {
        &self.writer
    }
    fn reader(&self) -> &ScriptedReader {
        &self.reader
    }
}
