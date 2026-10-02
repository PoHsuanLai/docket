//! A worker or run reported its end: one line in the front task's current task, and the orb
//! shows Waiting only if the result needs the person.

use agent_loop::{Attention, CompletionNote, LoopEffect, LoopPhase, LoopState, completion_line};
use companion_wire::{AnswerPhase, NeedsYou};

/// The effects of a completion for the front task. A line goes into a task that is still
/// working, or waiting for the person; a finished or absent front task gets none (the roster
/// and the episode carry it). A result that needs the person makes the front answer wait for
/// them.
pub fn completion_effects(note: &CompletionNote, front: Option<&LoopState>) -> Vec<LoopEffect> {
    let Some(line) = completion_line(note) else {
        return vec![];
    };
    let live = front.is_some_and(|s| !matches!(s.phase, LoopPhase::Finished(_)));
    if !live {
        return vec![];
    }
    let mut effects = vec![LoopEffect::Note(line.clone())];
    if note.attention == Attention::NeedsYou {
        effects.push(LoopEffect::Publish(AnswerPhase::NeedsYou(
            NeedsYou::Question {
                text: line,
                choices: vec![],
            },
        )));
    }
    effects
}
