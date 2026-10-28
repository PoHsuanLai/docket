//! "Always ask" and "never" in the router. A limit only tightens: the strongest covering limit
//! reaches the policy as a fact (`limit`), which asks or refuses, and no grant, standing grant or
//! approval lifts it back. The administrator's floor is consulted with
//! the person's own entries; the strongest wins. Only agents meet limits: the person's own
//! launcher acts never reach this file.

use crate::router::Router;
use crate::seams::{GrantStore, Seams};
use docket_core::{AppRefusal, FailText, Follow, Invocation, Outcome, Preview, Undoable};
use docket_gate::limits::{choice_arg, level_of, reach_of, text_arg};
use docket_gate::prepared::Prepared;
use docket_permissions_core::{
    Limit, LimitCall, LimitCaller, LimitKey, LimitLevel, LimitMaker, LimitScope, Limited, Said,
    Tighten, limited,
};
use docket_vocab::{ActionRef, Clock, LabelText, ParamName};
use porter_core::AppName;
use prov::{ActionName, Actor, SpaceScope};

fn failed(why: &str) -> AppRefusal {
    AppRefusal::Failed(FailText(why.to_owned()))
}

fn param(name: &str) -> ParamName {
    ParamName::parse(name).expect("a fixed parameter name is valid")
}

impl<S: Seams> Router<S> {
    /// What the limits (the person's, the assistant's and the floor's) say about `call`.
    pub(crate) fn limits_on(&self, call: &LimitCall<'_>) -> Limited {
        limited(&self.seams.grants().limits(), self.floor.locked(), call)
    }

    /// Writes `limit` as a rule in its file and puts the rebuilt policy in force, so the next call
    /// meets it.
    fn write_rule(&self, limit: &Limit) {
        self.seams.grants().add_rule(limit);
        self.reload_rules();
    }

    /// Puts in force the policy sets the rules files make now, after a rule was added or removed.
    pub(crate) fn reload_rules(&self) {
        if let Some(presets) = self.seams.grants().rebuilt() {
            self.set_presets(presets);
        }
    }

    /// The person said "never" on the sheet of `p`: the call is refused (the caller ends it)
    /// and the action is never asked about again in this Space, until the person removes it. It
    /// is a rule in the person's `never.cedar`.
    pub(crate) fn record_never(&self, p: &Prepared) {
        let key = LimitKey {
            caller: LimitCaller::Only(p.who.grant_caller()),
            scope: LimitScope::Action(p.request.action.clone()),
            space: SpaceScope::Only(p.space.clone()),
            from: p.decl.effect,
            level: LimitLevel::Never,
        };
        let limit = Limit::new(key, LimitMaker::Person, None, self.seams.clock().now());
        self.write_rule(&limit);
    }

    /// `companion.limit.add`: the assistant tightens on the person's word. Ask first or never,
    /// for an app, an action or everything, here or everywhere. There is no allow to give and no
    /// removal to ask for: only the person removes a limit, through the revoke path. The limit is
    /// a forbid rule in the assistant's rules file, which the loader holds to forbids.
    pub(crate) fn limit_add(&self, inv: &Invocation) -> Result<Outcome, AppRefusal> {
        let Actor::Companion { session, .. } = &inv.actor else {
            return Err(AppRefusal::Unsupported);
        };
        let level = level_of(choice_arg(inv, "level").as_deref()).ok_or_else(|| {
            AppRefusal::NeedsParam {
                param: param("level"),
                options: Vec::new(),
            }
        })?;
        let from = reach_of(choice_arg(inv, "reach").as_deref())
            .ok_or_else(|| failed("that is not a kind of change"))?;
        let app = text_arg(inv, "app")
            .map(|t| AppName::parse(&t).map_err(|_| failed("that is not an app name")))
            .transpose()?;
        let action = text_arg(inv, "action")
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
        let space = match choice_arg(inv, "space").as_deref() {
            Some("everywhere") => SpaceScope::Any,
            None | Some("here") => SpaceScope::Only(inv.space.clone()),
            Some(_) => return Err(failed("that is not a place")),
        };
        let said = self.words_the_person_said(session, text_arg(inv, "said"));
        let limit = Tighten {
            level,
            scope,
            space,
            from,
            said,
        }
        .limit(self.seams.clock().now());
        self.write_rule(&limit);
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
