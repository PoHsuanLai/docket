//! The host side of a confirm sheet, for a program that draws no window of its own.
//!
//! - [`ConfirmSheet`]: what a host implements to ask its person; the answer is the person's
//!   `SheetChoice`, or none for a sheet closed.
//! - [`SheetConfirmer`]: a [`ConfirmSheet`] as the router's `Confirmer`. It mints the receipt
//!   itself, so a sheet can say yes or no but a model can never forge a yes.
//! - [`EditorDesk`]: the sheets of an app whose person sits in an editor. The router asks it
//!   like any sheet and the request waits on the desk until the session host hands it to the
//!   editor and puts the editor's choice back.
//!
//! No bus, no runtime and no clock of its own: the clock that stamps receipts is passed in
//! (`docket_store::SystemClock` is the one system clock).

mod desk;
mod sheet;

pub use desk::EditorDesk;
pub use sheet::{ConfirmSheet, SheetConfirmer};
