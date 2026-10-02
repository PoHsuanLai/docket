//! Serving an app's provider on its own bus name.

use crate::provider::{ContextSource, IntentProvider, SummonTarget};
use crate::transport::TransportError;

/// Serves `org.quire.IntentProvider1` for `provider` on the app's activatable bus name until the
/// connection closes. The router calls it only after checking that the name's owner derives to
/// the same `AppId`.
pub async fn serve<P, C, S>(provider: P, context: C, summon: S) -> Result<(), TransportError>
where
    P: IntentProvider + 'static,
    C: ContextSource + 'static,
    S: SummonTarget + 'static,
{
    let _ = (provider, context, summon);
    todo!(
        "serve: claim the app's bus name, export IntentProviderSkeleton at /org/quire/IntentProvider1 over the three seams, push the manifest's index on demand"
    )
}
