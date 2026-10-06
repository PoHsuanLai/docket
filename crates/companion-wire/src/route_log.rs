//! One model turn's route: the events inferd sends before the reply, folded into notes.

use crate::route::{Reached, RouteNote, WhySays, WhyWord, why_says};
use porter_infer::{Declined, InferEvent, ModelLabel, ServedBy, StageRole, Why};

/// What a turn's events said about its route. Feed every event; read it when the turn ends.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RouteLog {
    notes: Vec<RouteNote>,
    words: Vec<WhyWord>,
    door: Option<Reached>,
    routed: Option<ServedBy>,
    declined: Option<Declined>,
}

impl RouteLog {
    /// Takes one event of the turn.
    pub fn event(&mut self, event: &InferEvent) {
        match event {
            InferEvent::Why(why) => self.said(why),
            InferEvent::Routed(served) => self.routed = Some(served.clone()),
            InferEvent::Declined(declined) => self.declined = Some(declined.clone()),
            InferEvent::Stage(stage) => {
                // The note's own `why` is always sent; the person sees it only when the reason
                // is shown, which is exactly when a `Why` event came first. Its door counts.
                if let WhySays::Door(reached) = why_says(&stage.why) {
                    self.door.get_or_insert(reached);
                }
                self.note(stage.role, stage.served.clone(), stage.name.clone());
            }
            _ => {}
        }
    }

    fn said(&mut self, why: &Why) {
        match why_says(why) {
            WhySays::Word(word) => self.words.push(word),
            WhySays::Door(reached) => self.door = Some(reached),
        }
    }

    fn note(&mut self, stage: StageRole, served: ServedBy, name: Option<ModelLabel>) {
        self.notes.push(RouteNote {
            stage,
            served,
            name,
            why: std::mem::take(&mut self.words),
            reached: self.door.take(),
        });
        self.routed = None;
    }

    /// The notes, oldest stage first. A turn whose inferd sent no `Stage` (one older than the
    /// every-answer note) is one unnamed `Answer` note made from `Routed` and the reasons before it.
    pub fn notes(mut self) -> Vec<RouteNote> {
        if let Some(served) = self.routed.take() {
            self.note(StageRole::Answer, served, None);
        }
        self.notes
    }

    /// The named model that could not serve, if the turn said so.
    pub fn declined(&self) -> Option<&Declined> {
        self.declined.as_ref()
    }
}
