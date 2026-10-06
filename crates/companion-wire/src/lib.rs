//! The wire of `org.quire.Companion1`: what an app or the launcher asks, the answer object
//! they watch (`AnswerWire`: a phase, a body, a footer), the cards, plans and forms inside it,
//! the records companiond stores about its sessions, and the front pointer (the task the
//! launcher returns to). Pure and serde-only; sill and the ds adapter read it, and quire
//! never names it (each pair of wire and view types has a `*_total` conversion test in sill).
//!
//! Session records are stored as `EventBody::Area { area: Companion }` in this serde form.

mod answer;
mod ask;
mod record;
mod route;
mod route_log;

pub use answer::{
    AnswerBody, AnswerPhase, AnswerWire, CardWire, FooterWire, FormWire, NeedsYou, PlanStepWire,
    PlanWire, RefusalWire, StepWireState,
};
pub use ask::{AskWire, FrontTask};
pub use record::{SESSION_KIND_PREFIX, SessionRecord};
pub use route::{
    Reached, RouteNote, WhySays, WhyWord, declined_text, footer_line, model_name, provider_name,
    why_says,
};
pub use route_log::RouteLog;
