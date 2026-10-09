//! Reporting what the person is looking at. ds knows the rows, selection and fields; the
//! window title and privacy come from the app. The adapter assembles a `ContextSnapshot` with
//! each text labelled by the kind's declared title trust.

use docket_client::ContextSource;
use docket_core::{
    ContextScope, ContextSnapshot, Here, Selection, TextTarget, TitleTrust, Visible, WindowPrivacy,
};
use ds_intents::ContextModel;
use porter_core::{AppName, Count};
use prov::{Label, Labelled, SpaceId};

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
    /// Reports the app alone, as a private window does: the mapping from `ContextModel` to
    /// `Here`, `Selection` and `Visible` is not built yet, and until it is the source says
    /// nothing about the window rather than something wrong (or, as it once did, panic). The
    /// router drops everything but the app from a `Private` snapshot, and no field is reported.
    fn snapshot(&self, _scope: ContextScope) -> ContextSnapshot {
        ContextSnapshot {
            app: self.app.clone(),
            window: Labelled::new(String::new(), Label::trusted_user()),
            here: Here::Nowhere,
            selection: Selection::Nothing,
            visible: Visible {
                kind: None,
                items: vec![],
                total: Count(0),
            },
            text_target: TextTarget::None,
            privacy: WindowPrivacy::Private,
        }
    }
}
