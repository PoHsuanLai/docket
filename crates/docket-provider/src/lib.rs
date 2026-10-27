//! The app side of `org.quire.Intents1`. An app implements [`IntentProvider`] and answers the
//! router's calls; `docket-ds` answers [`ContextSource`] and [`SummonTarget`] for a quire app.
//! With the `dbus` feature, [`serve`] and [`serve_on`] put the provider on the app's own bus name.
//! The caller side (`Intents` over a `Transport`) is `docket-client`.

#[cfg(feature = "dbus")]
mod bus;
#[cfg(feature = "dbus")]
mod error;
mod provider;
#[cfg(feature = "dbus")]
mod serve;

#[cfg(feature = "dbus")]
pub use bus::{serve_on, serve_on_with};
#[cfg(feature = "dbus")]
pub use error::ServeError;
pub use provider::{ContextSource, IntentProvider, SummonTarget};
#[cfg(feature = "dbus")]
pub use serve::serve;
