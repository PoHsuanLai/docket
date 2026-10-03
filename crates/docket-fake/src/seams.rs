//! `FakeLink` over the two fake apps, and `FakeSeams` bundling every fake into the router's
//! `Seams`.

use crate::files::FakeFiles;
use crate::mail::FakeMail;
use crate::scripted::{
    FakeMemory, ScriptedConfirmer, ScriptedReader, ScriptedReviewer, ScriptedWriter,
};
use crate::simple::{FixedClock, MemoryGrants, RecordingSink};
use docket_client::IntentProvider;
use docket_core::{
    AppRefusal, ContextScope, ContextSnapshot, EntityRef, Generation, Hit, Invocation, Latency,
    Outcome, Preview, SuggestAsk, UndoFault, UndoToken,
};
use docket_router::{AppFault, AppLink, LinkFault, Seams};
use porter_core::AppName;
use prov::{Actor, EntityId};
use std::collections::BTreeSet;
use std::sync::Mutex;

/// Whether a fake app answers `Perform`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answering {
    /// It answers.
    Normal,
    /// It never answers: the router times the call out.
    Silent,
}

/// Routes the router's calls to the fake apps by bus name.
#[derive(Debug)]
pub struct FakeLink {
    /// The mail app.
    pub mail: FakeMail,
    /// The files app.
    pub files: FakeFiles,
    /// Apps that never answer `Perform`: the call times out.
    pub silent: Mutex<BTreeSet<AppName>>,
    /// What the focused window shows, if a test set one: the fakes have no windows of their own.
    pub window: Mutex<Option<ContextSnapshot>>,
}

impl FakeLink {
    /// Both apps, answering.
    pub fn new(mail: FakeMail, files: FakeFiles) -> Self {
        Self {
            mail,
            files,
            silent: Mutex::new(BTreeSet::new()),
            window: Mutex::new(None),
        }
    }

    /// Makes `snapshot` what the app's `Context` answers (until the next call to this).
    pub fn show_window(&self, snapshot: ContextSnapshot) {
        if let Ok(mut window) = self.window.lock() {
            *window = Some(snapshot);
        }
    }

    /// Makes `app` stop answering `Perform` (its calls time out), or answer again.
    pub fn answer_from(&self, app: &AppName, answering: Answering) {
        if let Ok(mut silent) = self.silent.lock() {
            match answering {
                Answering::Silent => silent.insert(app.clone()),
                Answering::Normal => silent.remove(app),
            };
        }
    }

    fn is_mail(&self, app: &AppName) -> bool {
        self.mail.manifest().manifest().app == *app
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
        _within: Latency,
    ) -> Result<Outcome, AppFault> {
        if self.silent.lock().is_ok_and(|silent| silent.contains(app)) {
            return Err(AppFault::TimedOut);
        }
        if self.is_mail(app) {
            self.mail.perform(inv).await.map_err(AppFault::Refused)
        } else if self.is_files(app) {
            self.files.perform(inv).await.map_err(AppFault::Refused)
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

/// Every fake, public so a test reads what each recorded.
#[derive(Debug)]
pub struct FakeSeams {
    /// The apps.
    pub link: FakeLink,
    /// The sheet.
    pub confirmer: ScriptedConfirmer,
    /// The reviewer.
    pub reviewer: ScriptedReviewer,
    /// The consent store.
    pub grants: MemoryGrants,
    /// The event log.
    pub sink: RecordingSink,
    /// The clock.
    pub clock: FixedClock,
    /// Memory.
    pub memory: FakeMemory,
    /// The policy writer.
    pub writer: ScriptedWriter,
    /// The reader.
    pub reader: ScriptedReader,
}

impl Seams for FakeSeams {
    type Link = FakeLink;
    type Confirm = ScriptedConfirmer;
    type Review = ScriptedReviewer;
    type Grants = MemoryGrants;
    type Sink = RecordingSink;
    type Time = FixedClock;
    type Memory = FakeMemory;
    type Writer = ScriptedWriter;
    type Reading = ScriptedReader;

    fn link(&self) -> &FakeLink {
        &self.link
    }
    fn confirmer(&self) -> &ScriptedConfirmer {
        &self.confirmer
    }
    fn reviewer(&self) -> &ScriptedReviewer {
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
