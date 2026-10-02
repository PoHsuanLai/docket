//! The closed vocabulary of previews the host draws with its own components; apps send data only.

use crate::context::EntityRef;
use crate::ids::{FileRef, LabelText};
use crate::units::CharCount;
use porter_core::Bytes;
use prov::{Labelled, UnixSeconds};
use serde::{Deserialize, Serialize};

/// Text with light structure (paragraphs, emphasis), drawn as data.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Markdownish(pub String);

// It is the person's content: Debug shows the length only.
impl std::fmt::Debug for Markdownish {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Markdownish(<{} bytes>)", self.0.len())
    }
}

/// One line of a label and its value (a fact about a thing).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactLine {
    /// What the line is about, in the app's words.
    pub label: LabelText,
    /// The value.
    pub value: Labelled<String>,
}

/// A stretch of time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TimeRange {
    /// The start.
    pub from: UnixSeconds,
    /// The end.
    pub to: UnixSeconds,
}

/// One message in a thread preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSnip {
    /// Who wrote it.
    pub from: Labelled<String>,
    /// The first words.
    pub snippet: Labelled<String>,
    /// When.
    pub at: UnixSeconds,
}

/// A page of a PDF, counted from zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PageIndex(pub u32);

/// What is known about a file without opening it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileFacts {
    /// The file.
    pub file: FileRef,
    /// Its name.
    pub name: Labelled<String>,
    /// Its size.
    pub size: Bytes,
    /// Its media type.
    pub media_type: String,
}

/// One file moved from one place to another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMove {
    /// Where it is.
    pub from: FileRef,
    /// Where it goes.
    pub to: FileRef,
}

/// A preview the host draws.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Preview {
    /// Nothing to show.
    None,
    /// A heading and a body.
    Text {
        /// The heading.
        heading: Labelled<String>,
        /// The body.
        body: Labelled<Markdownish>,
    },
    /// Facts.
    Facts(Vec<FactLine>),
    /// A person.
    Person {
        /// Their name.
        name: Labelled<String>,
        /// Facts about them.
        lines: Vec<FactLine>,
    },
    /// A mail thread (at most five messages).
    Thread {
        /// The subject.
        subject: Labelled<String>,
        /// The messages.
        messages: Vec<MessageSnip>,
    },
    /// A calendar event.
    Event {
        /// The title.
        title: Labelled<String>,
        /// When.
        when: TimeRange,
        /// Where.
        place: Option<Labelled<String>>,
    },
    /// An image.
    Image(FileRef),
    /// A page of a PDF.
    Pdf {
        /// The file.
        file: FileRef,
        /// The page.
        page: PageIndex,
    },
    /// A file's facts.
    File(FileFacts),
    /// A list of things.
    List(Vec<EntityRef>),
    /// Inline replace: the text before and after.
    TextChange {
        /// Before.
        before: Labelled<String>,
        /// After.
        after: Labelled<String>,
    },
    /// Files that will move.
    Moves(Vec<FileMove>),
    /// A message that would be sent.
    Message {
        /// The recipients.
        to: Vec<Labelled<String>>,
        /// The subject.
        subject: Labelled<String>,
        /// The body.
        body: Labelled<String>,
    },
}

/// How much of a text a reviewer or a log is told: a length, never the text.
pub fn size_of(text: &str) -> CharCount {
    CharCount(u32::try_from(text.chars().count()).unwrap_or(u32::MAX))
}
