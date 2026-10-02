//! Test fakes for docket: the fixture manifests of a mail and a files app and in-memory
//! providers behind them (`FakeMail`, `FakeFiles`, with real undo tokens), scripted stand-ins
//! for everything that asks a person or a model (`ScriptedConfirmer`, `ScriptedReviewer`,
//! `ScriptedWriter`, `ScriptedReader`, `FakeMemory`), a fixed clock, a recording sink, an
//! in-memory consent store, and `fake_router` over all of them. Test code only: nothing in a
//! daemon links this crate.

mod files;
mod labels;
mod mail;
mod router;
mod scripted;
mod seams;
mod simple;

pub use files::FakeFiles;
pub use mail::{FakeMail, MailContact, MailThread, SentMail};
pub use router::{
    FILES_MANIFEST, FakeError, MAIL_MANIFEST, fake_router, files_manifest, mail_manifest, registry,
};
pub use scripted::{
    FakeMemory, ReviewMode, ScriptedConfirmer, ScriptedReader, ScriptedReviewer, ScriptedWriter,
};
pub use seams::{FakeLink, FakeSeams};
pub use simple::{FixedClock, MemoryGrants, RecordingSink};
