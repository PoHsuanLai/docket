//! The task policy of a session: derived from the person's own words, cut to what its parent
//! and its prompt field allow, narrowed silently and widened only when the person confirms.

use crate::router::Router;
use crate::seams::{Clock, EventSink, Seams};
use crate::state::RouterState;
use crate::tasks::child_policy;
use docket_core::{
    ActionCard, ActionDecl, ActionMatch, ActionRef, AgentReach, Anchor, ArgLine, AskReason,
    AuditRecord, ConfirmAnswer, ConfirmAnswerKind, ConfirmDetail, ConfirmEnd, ConfirmId,
    ConfirmOffer, ConfirmRequest, Confirmer, FileRef, Gesture, IntentsReply, LabelText,
    PolicyChange, PolicyWriter, Shown, TaintNote, TaskPolicy, TaskPolicyState, TrustedPattern,
    TurnSource, UserTurn, WidenAnswer, Widening, WireRefusal, Workspace, compare, intersection,
    tool_schema,
};
use porter_core::{AppName, Count};
use prov::{Effect, SessionId, UnixSeconds};
use std::collections::BTreeSet;

/// An action as a card: what the person reads, its effect and the schema of its arguments.
fn card_of(decl: &ActionDecl, app: &AppName) -> ActionCard {
    ActionCard {
        action: ActionRef {
            app: app.clone(),
            name: decl.name.clone(),
        },
        label: decl.label.clone(),
        effect: decl.effect,
        on: decl.on.clone(),
        tool: tool_schema(decl),
        reach: decl.reach,
        lasting: decl.lasting,
    }
}

/// The actions an agent may be offered, as cards: what the policy writer reads.
pub(crate) fn catalogue(st: &RouterState) -> Vec<ActionCard> {
    st.registry
        .all()
        .flat_map(|m| {
            let app = m.manifest().app.clone();
            m.manifest()
                .actions
                .iter()
                .filter(|a| a.reach != AgentReach::Hidden)
                .map(move |a| card_of(a, &app))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The policy's paths with each relative one made absolute under the session's directory: the
/// writer reads the person's words and cannot know where they were said. A relative path that
/// cannot be placed (no directory, or a `..` step that could climb out of it) is kept as it is:
/// it matches no file, so the bound it stands for still asks, where dropping it would lift the
/// bound.
fn anchored(paths: Vec<TrustedPattern>, cwd: Option<&Workspace>) -> Vec<TrustedPattern> {
    let place = |f: &FileRef| -> Option<FileRef> {
        let base = cwd?.as_str().trim_end_matches('/');
        let rest = f.as_str().trim_start_matches("./").trim_end_matches('/');
        let climbs = rest.split('/').any(|part| part == "..");
        FileRef::parse(&format!("{base}/{rest}"))
            .ok()
            .filter(|_| !climbs)
    };
    paths
        .into_iter()
        .map(|p| match &p {
            TrustedPattern::Under(f) if !f.as_str().starts_with('/') => {
                place(f).map_or(p.clone(), TrustedPattern::Under)
            }
            _ => p,
        })
        .collect()
}

/// A policy that covers nothing: what a first widening is measured against.
fn nothing(of: &TaskPolicy) -> TaskPolicy {
    TaskPolicy {
        actions: BTreeSet::new(),
        kinds: BTreeSet::new(),
        ceiling: Effect::Read,
        max_count: Count(0),
        recipients: vec![],
        destinations: vec![],
        paths: vec![],
        expires: UnixSeconds(0),
        ..of.clone()
    }
}

/// A prompt field's policy reaches its own app, and reads elsewhere.
fn cap_to_app(policy: TaskPolicy, app: &AppName, st: &RouterState) -> TaskPolicy {
    let actions = policy
        .actions
        .into_iter()
        .filter_map(|m| match m {
            ActionMatch::AppUpTo(a, e) if a == *app => Some(ActionMatch::AppUpTo(a, e)),
            ActionMatch::AppUpTo(a, e) => Some(ActionMatch::AppUpTo(a, e.min(Effect::Read))),
            ActionMatch::One(r) if r.app == *app => Some(ActionMatch::One(r)),
            ActionMatch::One(r) => st
                .registry
                .action(&r)
                .is_some_and(|d| d.effect == Effect::Read)
                .then_some(ActionMatch::One(r)),
        })
        .collect();
    TaskPolicy { actions, ..policy }
}

/// The widenings the person is asked about. A policy derived from the person's newest words
/// (`Baseline::Anything`) lives from that turn, so a later expiry than the last turn's policy is
/// the passing of time and not something to ask about: asking would put a sheet in front of a
/// person who asked for nothing more, and only when a clock second happened to tick between two
/// turns. A widening they confirm explicitly (`Baseline::Nothing`) is asked as it is.
fn asked_widenings(widenings: &[Widening], baseline: Baseline) -> Vec<Widening> {
    widenings
        .iter()
        .filter(|w| !(baseline == Baseline::Anything && **w == Widening::Expiry))
        .cloned()
        .collect()
}

impl<S: Seams> Router<S> {
    /// Cuts a policy to what its session may hold: stamped with the session's task, Space and
    /// turns, lapsing by the configured maximum, capped by its parent's policy and by the
    /// prompt field the last turn came from. An editor's typed turn is the person's words and is
    /// not capped, like a launcher's.
    fn bound_policy(
        &self,
        st: &RouterState,
        id: &SessionId,
        mut policy: TaskPolicy,
    ) -> Option<TaskPolicy> {
        let now = self.seams.clock().now();
        let record = st.sessions.get(id)?;
        policy.task = record.task.clone();
        policy.space = record.space.clone();
        policy.from = record.turns.iter().map(|t| t.id).collect();
        policy.state = TaskPolicyState::Active;
        policy.paths = anchored(policy.paths, record.cwd.as_ref());
        let latest = UnixSeconds(
            now.0
                .saturating_add(i64::from(self.agent_config().task_policy_max.0)),
        );
        policy.expires = policy.expires.min(latest);
        if let Some(TurnSource::Field(app)) = record.turns.last().map(|t| &t.from) {
            policy = cap_to_app(policy, app, st);
        }
        let parent = st
            .tasks
            .get(&record.task)
            .and_then(|t| t.parent.as_ref())
            .and_then(|p| st.sessions.values().find(|r| &r.task == p))
            .and_then(|r| r.policy.as_ref());
        Some(match parent {
            Some(parent) => child_policy(parent, &policy),
            None => policy,
        })
    }

    /// After the person speaks: asks the writer for a policy from their words alone and applies
    /// it. A writer that fails changes nothing: with no policy every non-read call is outside.
    pub(crate) async fn derive_policy(&self, id: &SessionId) {
        let (task, turns, catalogue, space) = {
            let st = self.locked();
            let Some(record) = st.sessions.get(id) else {
                return;
            };
            (
                record.task.clone(),
                record.turns.clone(),
                catalogue(&st),
                record.space.clone(),
            )
        };
        let Ok(derived) = self
            .seams
            .writer()
            .derive(&task, &turns, &catalogue, &space)
            .await
        else {
            return;
        };
        let derived = self.note_corrections(derived);
        let bounded = {
            let st = self.locked();
            self.bound_policy(&st, id, derived)
        };
        if let Some(policy) = bounded {
            self.apply_policy(id, policy, Baseline::Anything).await;
        }
    }

    /// Records each correction the writer made to its draft, and gives back the policy.
    fn note_corrections(&self, derived: docket_core::Derived) -> TaskPolicy {
        let at = self.seams.clock().now();
        for corrected in derived.corrected {
            self.seams.sink().append(AuditRecord::PolicyCorrected {
                at,
                task: derived.policy.task.clone(),
                corrected,
            });
        }
        derived.policy
    }

    /// `.Session.Narrow`: the person said something to a subagent. The policy writer reads it and
    /// the session's policy becomes what both the old policy and the new words allow, never more
    /// than either; a writer that fails changes nothing.
    pub(crate) async fn session_narrow(&self, id: &SessionId, turn: UserTurn) -> IntentsReply {
        let (task, catalogue, space, old) = {
            let st = self.locked();
            let Some(record) = st.sessions.get(id) else {
                return IntentsReply::Refused(WireRefusal::NoSuchSession);
            };
            (
                record.task.clone(),
                catalogue(&st),
                record.space.clone(),
                record.policy.clone(),
            )
        };
        let Ok(derived) = self
            .seams
            .writer()
            .derive(&task, &[turn], &catalogue, &space)
            .await
        else {
            return IntentsReply::Done;
        };
        let derived = self.note_corrections(derived);
        // Never more than the session's own policy: with none, the parent's bound applies.
        let narrowed = match old {
            Some(old) => Some(intersection(&old, &derived)),
            None => {
                let st = self.locked();
                self.bound_policy(&st, id, derived)
            }
        };
        if let Some(policy) = narrowed {
            self.apply_policy(id, policy, Baseline::Anything).await;
        }
        IntentsReply::Done
    }

    /// `.Session.Widen`: the person confirms a wider policy against their own words.
    pub(crate) async fn widen_policy(&self, id: &SessionId, wider: TaskPolicy) -> WidenAnswer {
        let bounded = {
            let st = self.locked();
            self.bound_policy(&st, id, wider)
        };
        match bounded {
            Some(policy) => self.apply_policy(id, policy, Baseline::Nothing).await,
            None => WidenAnswer::Refused(ConfirmEnd::Cancelled),
        }
    }

    /// Replaces the session's policy: silently when it covers no more than the old, after a
    /// confirmation quoting the person's last words when it covers more. `Baseline` says what
    /// a session with no policy is measured against.
    async fn apply_policy(
        &self,
        id: &SessionId,
        new: TaskPolicy,
        baseline: Baseline,
    ) -> WidenAnswer {
        let old = self
            .locked()
            .sessions
            .get(id)
            .and_then(|r| r.policy.clone());
        let change = match (&old, baseline) {
            (None, Baseline::Anything) => PolicyChange::Same,
            (None, Baseline::Nothing) => compare(&new, &nothing(&new)),
            (Some(old), _) => compare(&new, old),
        };
        if let PolicyChange::Widens(widenings) = &change {
            let asked = asked_widenings(widenings, baseline);
            if !asked.is_empty() {
                let answer = self.confirm_widening(id, &new, &asked).await;
                if let ConfirmAnswer::Ended(end) = answer {
                    return WidenAnswer::Refused(end);
                }
            }
        }
        let at = self.seams.clock().now();
        let task = new.task.clone();
        if let Some(record) = self.locked().sessions.get_mut(id) {
            record
                .wal
                .note(docket_session::SessionEntry::Policy(new.clone()));
            record.policy = Some(new);
        }
        self.seams.sink().append(AuditRecord::TaskPolicy {
            at,
            task,
            state: TaskPolicyState::Active,
            change: change.kind(),
        });
        WidenAnswer::Applied
    }

    async fn confirm_widening(
        &self,
        id: &SessionId,
        new: &TaskPolicy,
        widenings: &[Widening],
    ) -> ConfirmAnswer {
        let request = {
            let mut st = self.locked();
            let n = st.mint();
            let (Ok(cid), Some(record)) =
                (ConfirmId::parse(&format!("c-{n}")), st.sessions.get(id))
            else {
                return ConfirmAnswer::Ended(ConfirmEnd::Cancelled);
            };
            let said = record
                .turns
                .last()
                .map(|t| t.text.clone())
                .unwrap_or_default();
            let mut lines = vec![ArgLine {
                label: words("You said"),
                value: Shown::Plain(said),
            }];
            lines.extend(widenings.iter().map(|w| ArgLine {
                label: words("Also allows"),
                value: Shown::Plain(describe(w)),
            }));
            let request = ConfirmRequest {
                id: cid.clone(),
                space: new.space.clone(),
                actor: record.actor.clone(),
                app: record.opener.clone(),
                action: words("Allow more for this task"),
                effect: new.ceiling,
                count: new.max_count,
                detail: ConfirmDetail::Plain,
                lines,
                why: vec![AskReason::OutsideTask],
                taint: TaintNote::Clean,
                offer: ConfirmOffer::OnceOnly,
                always: Default::default(),
                gesture: Gesture::Press,
                anchor: Anchor::Launcher,
                expires: self.agent_config().confirm_expiry,
                editor: crate::who::editor_of(record).map(|client| docket_core::EditorRoute {
                    client,
                    session: id.clone(),
                }),
            };
            st.pending.insert(cid, new.space.clone());
            request
        };
        let cid = request.id.clone();
        let answer = self
            .seams
            .confirmer()
            .confirm(request)
            .await
            .without_terminal_grant();
        self.locked().pending.remove(&cid);
        let (kind, input) = match &answer {
            ConfirmAnswer::Allowed { scope, receipt } => (
                ConfirmAnswerKind::Allowed(scope.clone()),
                Some(receipt.input),
            ),
            ConfirmAnswer::AllowedFromTerminal { receipt } => {
                (ConfirmAnswerKind::AllowedFromTerminal, Some(receipt.input))
            }
            ConfirmAnswer::Ended(end) => (ConfirmAnswerKind::Ended(*end), None),
        };
        self.seams.sink().append(AuditRecord::Confirm {
            at: self.seams.clock().now(),
            id: cid,
            answer: kind,
            input,
        });
        answer
    }
}

/// What a session with no policy yet counts as when a new one is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Baseline {
    /// The first policy a turn derives is applied as it is: it is the person's own words.
    Anything,
    /// A widening the person asked for is measured against nothing, so it confirms.
    Nothing,
}

fn words(text: &str) -> LabelText {
    LabelText::parse(text).expect("fixed sheet words are valid label text")
}

fn describe(w: &Widening) -> String {
    match w {
        Widening::Action(ActionMatch::One(a)) => a.name.to_string(),
        Widening::Action(ActionMatch::AppUpTo(app, e)) => format!("{app} up to {e:?}"),
        Widening::Kind(k) => k.to_string(),
        Widening::Ceiling(e) => format!("effects up to {e:?}"),
        Widening::Count(c) => format!("up to {} things at once", c.0),
        Widening::Pattern(sink, _) => format!("a new {sink:?}"),
        Widening::Expiry => "for longer".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prov::Effect;

    fn under(path: &str) -> TrustedPattern {
        TrustedPattern::Under(FileRef::parse(path).expect("file"))
    }

    #[test]
    fn a_relative_path_is_placed_under_the_session_directory_or_kept_as_it_is() {
        let cwd = Workspace::parse("/home/u/proj").expect("cwd");
        let table = [
            ("tests/", Some(&cwd), "/home/u/proj/tests"),
            ("./tests", Some(&cwd), "/home/u/proj/tests"),
            ("/srv/data", Some(&cwd), "/srv/data"),
            ("../other", Some(&cwd), "../other"),
            ("tests", None, "tests"),
        ];
        for (written, at, want) in table {
            assert_eq!(
                anchored(vec![under(written)], at),
                vec![under(want)],
                "{written}"
            );
        }
    }

    #[test]
    fn a_later_expiry_alone_is_not_asked_about_for_the_words_of_a_new_turn() {
        let both = [Widening::Expiry, Widening::Ceiling(Effect::Outbound)];
        let table = [
            (
                Baseline::Anything,
                &both[..],
                vec![Widening::Ceiling(Effect::Outbound)],
            ),
            (Baseline::Anything, &both[..1], vec![]),
            (Baseline::Nothing, &both[..1], vec![Widening::Expiry]),
            (Baseline::Nothing, &both[..], both.to_vec()),
        ];
        for (baseline, widenings, asked) in table {
            assert_eq!(asked_widenings(widenings, baseline), asked, "{baseline:?}");
        }
    }
}
