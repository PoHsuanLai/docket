//! The proposed values the specs name, as one typed configuration with the defaults, and the
//! table of settings rows that carry them. docket never reads settings itself: sill's
//! Intelligence page owns the keys (design/22 rows), and the daemons receive an
//! [`AgentConfig`].

use crate::budget::Budget;
use crate::review::Strictness;
use crate::units::{Depth, Millis, Seconds};
use porter_core::{Count, MicroUsd, Tokens};
use serde::{Deserialize, Serialize};

/// When the breaker trips (`agent.breaker.*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BreakerLimits {
    /// Denials in a row (3).
    pub consecutive: Count,
    /// Denials among the last `window` decisions (10).
    pub recent: Count,
    /// How many decisions the recent count looks back over (50).
    pub window: Count,
    /// Different arguments for one goal before it counts as probing (3).
    pub probing: Count,
}

/// How long each review stage may take before it counts as a failure and asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReviewTimeouts {
    /// `agent.review.quick_ms` (300).
    pub quick: Millis,
    /// `agent.review.deliberate_ms` (3000).
    pub deliberate: Millis,
    /// `agent.review.second_ms` (3000).
    pub second: Millis,
}

/// How many tokens each section of the planner's working set may use
/// (`companion.budget.*`, measured against the real tokenizer later). The sections run from
/// most stable to least so the engine reuses its cached prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AssemblerBudget {
    /// 1: rules, persona and the action cards (2500).
    pub rules: Tokens,
    /// 2: profile facts, primer and the latest rollup (1200).
    pub profile: Tokens,
    /// 3: the roster (400).
    pub roster: Tokens,
    /// 4: recent episodes (1200).
    pub episodes: Tokens,
    /// 5: recall (1500).
    pub recall: Tokens,
    /// 6: the context snapshot (1000).
    pub context: Tokens,
    /// 7: the current task (3000).
    pub task: Tokens,
    /// How many of the newest steps stay in full; older outcomes are masked (3).
    pub full_steps: Count,
}

/// When the companion's background work runs and when a side conversation ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IdleRules {
    /// No interactive request for this long starts the idle pass (30 s).
    pub idle_after: Seconds,
    /// A side conversation with a subagent ends after this long without a turn (120 s).
    pub side_close: Seconds,
}

/// Every proposed value in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentConfig {
    /// `agent.strictness`.
    pub strictness: Strictness,
    /// `agent.budget.*`.
    pub budget: Budget,
    /// `agent.mass_at`: more things than this in one call asks (20).
    pub mass_at: Count,
    /// `agent.confirm.expiry_s` (120).
    pub confirm_expiry: Seconds,
    /// `agent.task_policy.max_s`: a task policy lapses by then, or at task end (3600).
    pub task_policy_max: Seconds,
    /// `agent.undo.keep_h` in seconds (24 h).
    pub undo_keep: Seconds,
    /// `agent.breaker.*`.
    pub breaker: BreakerLimits,
    /// `agent.review.*`.
    pub review: ReviewTimeouts,
    /// `companion.budget.*`.
    pub assembler: AssemblerBudget,
    /// `companion.idle_s` and `companion.side_close_s`.
    pub idle: IdleRules,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            strictness: Strictness::Default,
            budget: Budget {
                calls: Count(200),
                writes: Count(100),
                outbound: Count(10),
                destructive: Count(5),
                per_minute: Count(30),
                fan_out: Count(50),
                chain: Depth(4),
                wall: Seconds(1800),
                reviews: Count(300),
                spend: MicroUsd(0),
            },
            mass_at: Count(20),
            confirm_expiry: Seconds(120),
            task_policy_max: Seconds(3600),
            undo_keep: Seconds(24 * 3600),
            breaker: BreakerLimits {
                consecutive: Count(3),
                recent: Count(10),
                window: Count(50),
                probing: Count(3),
            },
            review: ReviewTimeouts {
                quick: Millis(300),
                deliberate: Millis(3000),
                second: Millis(3000),
            },
            assembler: AssemblerBudget {
                rules: Tokens(2500),
                profile: Tokens(1200),
                roster: Tokens(400),
                episodes: Tokens(1200),
                recall: Tokens(1500),
                context: Tokens(1000),
                task: Tokens(3000),
                full_steps: Count(3),
            },
            idle: IdleRules {
                idle_after: Seconds(30),
                side_close: Seconds(120),
            },
        }
    }
}

/// A settings value as a row carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingValue {
    /// A number in the row's unit.
    Number(i64),
    /// One word of a closed set.
    Word(&'static str),
}

/// One settings row docket's daemons read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingRow {
    /// The key.
    pub key: &'static str,
    /// The proposed default.
    pub default: SettingValue,
}

const fn n(key: &'static str, v: i64) -> SettingRow {
    SettingRow {
        key,
        default: SettingValue::Number(v),
    }
}

/// Every row, in the order the Intelligence page lists them. The retention row `companion.*`
/// (30 days) is almanac's `RuleSet::standard()`; the desktop-scope join rule is `prov`'s.
pub const SETTING_ROWS: &[SettingRow] = &[
    SettingRow {
        key: "agent.strictness",
        default: SettingValue::Word("default"),
    },
    n("agent.budget.calls", 200),
    n("agent.budget.writes", 100),
    n("agent.budget.outbound", 10),
    n("agent.budget.destructive", 5),
    n("agent.budget.per_minute", 30),
    n("agent.budget.fan_out", 50),
    n("agent.budget.chain", 4),
    n("agent.budget.wall_s", 1800),
    n("agent.budget.reviews", 300),
    n("agent.mass_at", 20),
    n("agent.confirm.expiry_s", 120),
    n("agent.task_policy.max_s", 3600),
    n("agent.undo.keep_h", 24),
    n("agent.breaker.consecutive", 3),
    n("agent.breaker.recent", 10),
    n("agent.review.quick_ms", 300),
    n("agent.review.deliberate_ms", 3000),
    n("agent.review.second_ms", 3000),
    n("companion.budget.rules", 2500),
    n("companion.budget.profile", 1200),
    n("companion.budget.roster", 400),
    n("companion.budget.episodes", 1200),
    n("companion.budget.recall", 1500),
    n("companion.budget.context", 1000),
    n("companion.budget.task", 3000),
    n("companion.budget.full_steps", 3),
    n("companion.idle_s", 30),
    n("companion.side_close_s", 120),
];

impl AgentConfig {
    /// What this configuration holds for `key`, in the row's unit; none for an unknown key.
    pub fn value(&self, key: &str) -> Option<SettingValue> {
        let num = |v: i64| Some(SettingValue::Number(v));
        let c = |count: Count| i64::from(count.0);
        let t = |tokens: Tokens| i64::from(tokens.0);
        match key {
            "agent.strictness" => Some(SettingValue::Word(match self.strictness {
                Strictness::AskMore => "ask_more",
                Strictness::Default => "default",
                Strictness::TrustMore => "trust_more",
            })),
            "agent.budget.calls" => num(c(self.budget.calls)),
            "agent.budget.writes" => num(c(self.budget.writes)),
            "agent.budget.outbound" => num(c(self.budget.outbound)),
            "agent.budget.destructive" => num(c(self.budget.destructive)),
            "agent.budget.per_minute" => num(c(self.budget.per_minute)),
            "agent.budget.fan_out" => num(c(self.budget.fan_out)),
            "agent.budget.chain" => num(i64::from(self.budget.chain.0)),
            "agent.budget.wall_s" => num(i64::from(self.budget.wall.0)),
            "agent.budget.reviews" => num(c(self.budget.reviews)),
            "agent.mass_at" => num(c(self.mass_at)),
            "agent.confirm.expiry_s" => num(i64::from(self.confirm_expiry.0)),
            "agent.task_policy.max_s" => num(i64::from(self.task_policy_max.0)),
            "agent.undo.keep_h" => num(i64::from(self.undo_keep.0 / 3600)),
            "agent.breaker.consecutive" => num(c(self.breaker.consecutive)),
            "agent.breaker.recent" => num(c(self.breaker.recent)),
            "agent.review.quick_ms" => num(i64::from(self.review.quick.0)),
            "agent.review.deliberate_ms" => num(i64::from(self.review.deliberate.0)),
            "agent.review.second_ms" => num(i64::from(self.review.second.0)),
            "companion.budget.rules" => num(t(self.assembler.rules)),
            "companion.budget.profile" => num(t(self.assembler.profile)),
            "companion.budget.roster" => num(t(self.assembler.roster)),
            "companion.budget.episodes" => num(t(self.assembler.episodes)),
            "companion.budget.recall" => num(t(self.assembler.recall)),
            "companion.budget.context" => num(t(self.assembler.context)),
            "companion.budget.task" => num(t(self.assembler.task)),
            "companion.budget.full_steps" => num(c(self.assembler.full_steps)),
            "companion.idle_s" => num(i64::from(self.idle.idle_after.0)),
            "companion.side_close_s" => num(i64::from(self.idle.side_close.0)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_settings_row_carries_the_default_of_the_configuration() {
        let config = AgentConfig::default();
        for row in SETTING_ROWS {
            assert_eq!(config.value(row.key), Some(row.default), "row {}", row.key);
        }
    }

    #[test]
    fn row_keys_are_unique_and_namespaced() {
        let mut seen = std::collections::BTreeSet::new();
        for row in SETTING_ROWS {
            assert!(seen.insert(row.key), "duplicate {}", row.key);
            assert!(
                row.key.starts_with("agent.") || row.key.starts_with("companion."),
                "{}",
                row.key
            );
        }
    }

    #[test]
    fn the_assembler_budget_sums_to_the_working_set_of_the_research() {
        let a = AgentConfig::default().assembler;
        let total = a.rules.0
            + a.profile.0
            + a.roster.0
            + a.episodes.0
            + a.recall.0
            + a.context.0
            + a.task.0;
        assert_eq!(total, 10_800);
    }
}
