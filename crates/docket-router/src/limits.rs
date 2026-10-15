//! "Always ask" and "never" in the router. A limit only tightens: after the gate has decided,
//! a covering limit turns a call that would run into one that asks, or into a refusal, and no
//! grant, standing grant or approval lifts it back. The administrator's floor is consulted with
//! the person's own entries; the strongest wins. Only agents meet limits: the person's own
//! launcher acts never reach this file.

use crate::gate::Pending;
use crate::prepared::Prepared;
use crate::router::Router;
use crate::seams::{Clock, GrantStore, Seams};
use docket_core::{
    ActionRef, AppRefusal, AskReason, CallRefusal, DenyCode, FailText, Follow, Invocation,
    LabelText, Limit, LimitCall, LimitCaller, LimitKey, LimitLevel, LimitMaker, LimitScope,
    Limited, Outcome, ParamName, Preview, Said, Tighten, Undoable, Value, limited,
};
use porter_core::AppName;
use prov::{ActionName, Actor, Effect, SpaceScope};

/// The assistant's one action that adds a limit.
pub(crate) const LIMIT_ADD: &str = "companion.limit.add";

/// `pending` under what the limits say. A refusal that came first stands (a halt, a budget): the
/// limit adds nothing to it.
pub fn tightened(pending: Pending, limit: &Limited) -> Pending {
    match (limit, pending) {
        (_, refused @ Pending::Refuse(_)) => refused,
        (Limited::Free, other) => other,
        (Limited::Never(_), _) => Pending::Refuse(CallRefusal::Denied(DenyCode::Never)),
        (Limited::Ask(_), Pending::Confirm(mut why)) => {
            if !why.contains(&AskReason::AskAlways) {
                why.push(AskReason::AskAlways);
            }
            Pending::Confirm(why)
        }
        (Limited::Ask(_), Pending::Run | Pending::NeedsReview(_)) => {
            Pending::Confirm(vec![AskReason::AskAlways])
        }
    }
}

fn failed(why: &str) -> AppRefusal {
    AppRefusal::Failed(FailText(why.to_owned()))
}

fn param(name: &str) -> ParamName {
    ParamName::parse(name).expect("a fixed parameter name is valid")
}

fn text(inv: &Invocation, name: &str) -> Option<String> {
    match inv.args.get(&param(name)).map(|a| &a.value) {
        Some(Value::Text(t)) if !t.trim().is_empty() => Some(t.clone()),
        _ => None,
    }
}

fn choice(inv: &Invocation, name: &str) -> Option<String> {
    match inv.args.get(&param(name)).map(|a| &a.value) {
        Some(Value::Choice(c)) => Some(c.to_string()),
        _ => None,
    }
}

/// The level the `level` choice names: `ask` or `never`. Nothing else is a level, so the
/// action cannot carry an allow.
fn level_of(choice: Option<&str>) -> Option<LimitLevel> {
    match choice {
        Some("ask") => Some(LimitLevel::AlwaysAsk),
        Some("never") => Some(LimitLevel::Never),
        _ => None,
    }
}

/// The weakest effect the `reach` choice covers.
fn reach_of(choice: Option<&str>) -> Option<Effect> {
    match choice {
        None | Some("changes") => Some(Effect::UndoableWrite),
        Some("anything") => Some(Effect::Read),
        Some("removals") => Some(Effect::Destructive),
        Some(_) => None,
    }
}

impl<S: Seams> Router<S> {
    /// What the limits (the person's, the assistant's and the floor's) say about `call`.
    pub(crate) fn limits_on(&self, call: &LimitCall<'_>) -> Limited {
        limited(&self.seams.grants().limits(), self.floor.locked(), call)
    }

    /// The person said "never" on the sheet of `p`: the call is refused (the caller ends it)
    /// and the action is never asked about again in this Space, until the person removes it.
    pub(crate) fn record_never(&self, p: &Prepared) {
        let key = LimitKey {
            caller: LimitCaller::Only(p.who.grant_caller()),
            scope: LimitScope::Action(p.request.action.clone()),
            space: SpaceScope::Only(p.space.clone()),
            from: p.decl.effect,
            level: LimitLevel::Never,
        };
        let limit = Limit::new(key, LimitMaker::Person, None, self.seams.clock().now());
        self.seams.grants().add_limit(limit);
    }

    /// `companion.limit.add`: the assistant tightens on the person's word. Ask first or never,
    /// for an app, an action or everything, here or everywhere. There is no allow to give and no
    /// removal to ask for: only the person removes a limit, through the revoke path.
    pub(crate) fn limit_add(&self, inv: &Invocation) -> Result<Outcome, AppRefusal> {
        let Actor::Companion { session, .. } = &inv.actor else {
            return Err(AppRefusal::Unsupported);
        };
        let level =
            level_of(choice(inv, "level").as_deref()).ok_or_else(|| AppRefusal::NeedsParam {
                param: param("level"),
                options: Vec::new(),
            })?;
        let from = reach_of(choice(inv, "reach").as_deref())
            .ok_or_else(|| failed("that is not a kind of change"))?;
        let app = text(inv, "app")
            .map(|t| AppName::parse(&t).map_err(|_| failed("that is not an app name")))
            .transpose()?;
        let action = text(inv, "action")
            .map(|t| ActionName::parse(&t).map_err(|_| failed("that is not an action name")))
            .transpose()?;
        let scope = match (app, action) {
            (None, None) => LimitScope::Everything,
            (Some(app), None) => LimitScope::App(app),
            (Some(app), Some(name)) => {
                let named = ActionRef { app, name };
                if self.locked().registry.action(&named).is_none() {
                    return Err(failed("there is no such action"));
                }
                LimitScope::Action(named)
            }
            (None, Some(_)) => return Err(failed("an action needs its app")),
        };
        let space = match choice(inv, "space").as_deref() {
            Some("everywhere") => SpaceScope::Any,
            None | Some("here") => SpaceScope::Only(inv.space.clone()),
            Some(_) => return Err(failed("that is not a place")),
        };
        let said = self.words_the_person_said(session, text(inv, "said"));
        let limit = Tighten {
            level,
            scope,
            space,
            from,
            said,
        }
        .limit(self.seams.clock().now());
        self.seams.grants().add_limit(limit);
        Ok(Outcome {
            value: None,
            said: LabelText::parse(match level {
                LimitLevel::AlwaysAsk => "Will ask first",
                LimitLevel::Never => "Will not do that",
            })
            .ok(),
            show: Preview::None,
            undo: Undoable::No,
            follow: Follow::Nothing,
        })
    }

    /// `words`, kept only when the person really said them in this session: the assistant may
    /// quote, and a quote that is not in the person's turns is dropped, so the reason shown
    /// is then a plain phrase made from the limit.
    fn words_the_person_said(
        &self,
        session: &prov::SessionId,
        words: Option<String>,
    ) -> Option<Said> {
        let said = Said::parse(&words?).ok()?;
        let wanted = said.as_str().to_lowercase();
        let st = self.locked();
        let record = st.sessions.get(session)?;
        record
            .turns
            .iter()
            .any(|t| t.text.to_lowercase().contains(&wanted))
            .then_some(said)
    }
}

/// Whether a call is the assistant's own `companion.limit.add`: the one action that needs no
/// first-use question, because it can only make things stricter.
pub(crate) fn is_limit_add(app: &AppName, name: &ActionName) -> bool {
    app.as_str() == crate::companion::COMPANION_APP && name.as_str() == LIMIT_ADD
}
