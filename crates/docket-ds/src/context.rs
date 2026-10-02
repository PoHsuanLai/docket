//! Reporting what the person is looking at. ds knows the rows, selection and fields; the
//! window title and privacy come from the app. The adapter assembles a `ContextSnapshot` with
//! each text labelled by the kind's declared title trust.

use docket_client::ContextSource;
use docket_core::{ContextScope, ContextSnapshot, TitleTrust};
use ds_intents::ContextModel;
use porter_core::AppName;
use prov::SpaceId;

/// What only the app knows about its window.
pub trait WindowFacts: Send + Sync {
    /// The window's title.
    fn title(&self) -> String;
    /// Whether the window is private (the router then drops everything but the app).
    fn is_private(&self) -> docket_core::WindowPrivacy;
    /// The Space the window lives in.
    fn space(&self) -> SpaceId;
    /// How the kind of thing the person looks at is titled (who writes its words).
    fn titles_of(&self, kind: &str) -> TitleTrust;
}

/// The `ContextSource` of a quire app, over ds's `ContextModel`.
#[derive(Debug)]
pub struct DsContextSource<M: ContextModel, W: WindowFacts> {
    app: AppName,
    model: M,
    window: W,
}

impl<M: ContextModel, W: WindowFacts> DsContextSource<M, W> {
    /// Reports the context of `app` from `model` and `window`.
    pub fn new(app: AppName, model: M, window: W) -> Self {
        Self { app, model, window }
    }
}

impl<M: ContextModel + Send + Sync, W: WindowFacts> ContextSource for DsContextSource<M, W> {
    fn snapshot(&self, scope: ContextScope) -> ContextSnapshot {
        let _ = (scope, &self.app, &self.model, &self.window);
        todo!(
            "DsContextSource::snapshot: ContextModel::thing and things become Here, Selection and Visible through entity_ref; the window title is labelled by the app; a private window reports the app alone; Password and PIN fields are never reported"
        )
    }
}
