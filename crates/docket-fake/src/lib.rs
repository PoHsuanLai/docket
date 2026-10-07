//! Test fakes for docket: the fixture manifests of a mail and a files app and in-memory
//! providers behind them (`FakeMail`, `FakeFiles`, with real undo tokens), scripted stand-ins
//! for everything that asks a person or a model (`ScriptedConfirmer`, `ScriptedReviewer`,
//! `ScriptedWriter`, `ScriptedReader`, `FakeMemory`), a fixed clock, a recording sink, an
//! in-memory consent store, and `fake_router` over all of them. Test code only: nothing in a
//! daemon links this crate.

mod clock;
mod files;
mod labels;
mod mail;
mod menu;
mod parsed;
mod router;
mod scripted;
mod seams;
mod simple;

pub use clock::FixedClock;
pub use files::FakeFiles;
pub use mail::{FakeMail, MailContact, MailThread, SentMail};
pub use menu::{FakeMenu, MenuItem, MenuPerform};
pub use parsed::ParsedReviewer;
pub use router::{
    FILES_MANIFEST, FakeError, MAIL_MANIFEST, MENU_MANIFEST, fake_router, fake_router_on,
    fake_router_with, files_manifest, install_menu, mail_manifest, menu_manifest, registry,
};
pub use scripted::{
    FakeMemory, Forget, ReviewMode, ScriptedConfirmer, ScriptedReader, ScriptedReviewer,
    ScriptedWriter,
};
pub use seams::Answering;
pub use seams::{FakeLink, FakeSeams, host_companion};
pub use simple::{MemoryGrants, RecordingSink};
