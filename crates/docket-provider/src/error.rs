//! Why a provider could not be served.

/// Why serving a provider on the bus failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ServeError {
    /// The bus refused or failed.
    #[error("bus")]
    Bus(#[from] zbus::Error),
    /// Another process already owns the app's bus name.
    #[error("{0} is taken")]
    Taken(String),
}
