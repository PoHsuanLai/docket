//! The options apps offer for their dynamic parameters. Before a planner turn, each dynamic
//! parameter of an action the view offers is asked once (`Suggest`, nothing typed) through the
//! router, and the answers go to the planner as data, so its schema lists them. A parameter whose
//! app does not answer in time, refuses or offers nothing stays free text.

use crate::deadline::timed;
use crate::runtime::Companion;
use crate::seams::Surface;
use docket_client::Transport as IntentsTransport;
use docket_core::{PlannerView, SuggestAsk};
use docket_planner::ParamChoices;
use docket_planner::Planner;
use docket_vocab::{Clock, ParamType};
use std::time::Duration;

/// How long an app has to offer its options: the turn is waiting on it.
const OFFER_WAIT: Duration = Duration::from_secs(2);

impl<P: Planner, I: IntentsTransport, K: Clock, S: Surface> Companion<P, I, K, S> {
    /// Asks each dynamic parameter of the view's actions for its options and hands what came
    /// back to the planner, replacing the last turn's.
    pub(crate) async fn offer_choices(&mut self, view: &PlannerView) {
        let mut offered: Vec<ParamChoices> = Vec::new();
        for card in view.actions.iter().filter(|c| c.related.is_empty()) {
            let Some(tool) = self.planner.catalogue().of(&card.action) else {
                continue;
            };
            let action = tool.action();
            let dynamic: Vec<_> = tool
                .decl
                .params
                .iter()
                .filter(|p| matches!(p.ty, ParamType::Dynamic(_)))
                .map(|p| p.name.clone())
                .collect();
            for param in dynamic {
                let ask = SuggestAsk {
                    action: action.clone(),
                    param: param.clone(),
                    typed: String::new(),
                };
                let asked = timed(&self.clock, OFFER_WAIT, self.intents.suggest(ask)).await;
                let Ok(Ok(things)) = asked else {
                    continue;
                };
                let keys = things
                    .iter()
                    .map(|t| t.id.key.as_str().to_owned())
                    .collect();
                offered.extend(ParamChoices::new(action.clone(), param, keys));
            }
        }
        self.planner.set_choices(offered);
    }
}
