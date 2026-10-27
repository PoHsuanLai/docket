//! The proposed values the specs name, as one typed configuration with the defaults, and the
//! table of settings rows that carry them. docket never reads settings itself: sill's
//! Intelligence page owns the keys (design/22 rows), and the daemons receive an
//! [`AgentConfig`].

use crate::budget::Budget;
use crate::mode::{Mode, RowSets};
use crate::units::{Days, Depth, Millis, Seconds};
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
    /// The quick stage.
    pub quick: Millis,
    /// The careful stage.
    pub deliberate: Millis,
    /// The second opinion.
    pub second: Millis,
}

impl ReviewTimeouts {
    /// What the product uses: 300 ms for the quick stage, 3000 ms for each of the others.
    pub const STANDARD: Self = Self {
        quick: Millis(300),
        deliberate: Millis(3000),
        second: Millis(3000),
    };
}

impl Default for ReviewTimeouts {
    fn default() -> Self {
        Self::STANDARD
    }
}

/// A task policy lapses by then, or at task end (one hour).
pub const TASK_POLICY_MAX: Seconds = Seconds(3600);

/// How long the undo history keeps what the assistant did (24 hours).
pub const UNDO_KEEP: Seconds = Seconds(24 * 3600);

/// What one session may spend. Zero spend keeps a task on this computer: a local model costs
/// nothing, and a paid turn is over the cap until the person raises it (`agent.budget.spend_microusd`).
pub const STANDARD_BUDGET: Budget = Budget {
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
};

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

fn proceed_within() -> Millis {
    Millis(10_000)
}

fn checkpoint_keep() -> Count {
    Count(20)
}

fn checkpoint_days() -> Days {
    Days(14)
}

fn checkpoint_max_files() -> Count {
    Count(50_000)
}

fn checkpoint_wait() -> Seconds {
    Seconds(10)
}

fn checkpoint_turn_max() -> Seconds {
    Seconds(1800)
}

/// Every proposed value in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentConfig {
    /// `agent.start_mode`: the mode a new conversation starts in (an old `strictness` or
    /// `permissions` still reads).
    #[serde(default, alias = "strictness", alias = "permissions")]
    pub start_mode: Mode,
    /// `agent.auto.*` and `agent.manual.*`: what each mode does without asking.
    #[serde(default)]
    pub rows: RowSets,
    /// How long each review stage may take. Not a setting: it starts at
    /// [`ReviewTimeouts::STANDARD`], and a test or an evaluation run may set another.
    #[serde(default)]
    pub review: ReviewTimeouts,
    /// What one session may spend; only `agent.budget.spend_microusd` is a setting.
    pub budget: Budget,
    /// `agent.mass_at`: more things than this in one call asks (20).
    pub mass_at: Count,
    /// `agent.confirm.expiry_s` (120).
    pub confirm_expiry: Seconds,
    /// `agent.confirm.proceed_ms`: how long a watching requester has to say `Proceed` before a
    /// step that must ask the person is refused unasked (10 s).
    #[serde(default = "proceed_within")]
    pub confirm_proceed: Millis,
    /// `agent.breaker.*`.
    pub breaker: BreakerLimits,
    /// `agent.checkpoints.keep`: restore points kept per session (20).
    #[serde(default = "checkpoint_keep")]
    pub checkpoint_keep: Count,
    /// `agent.checkpoints.days`: how long a restore point is kept (14 days).
    #[serde(default = "checkpoint_days")]
    pub checkpoint_days: Days,
    /// `agent.checkpoints.max_files`: a folder with more files than this is not saved (50 000).
    #[serde(default = "checkpoint_max_files")]
    pub checkpoint_max_files: Count,
    /// `agent.checkpoints.wait_s`: how long a turn waits for its restore point before going on
    /// without one (10 s).
    #[serde(default = "checkpoint_wait")]
    pub checkpoint_wait: Seconds,
    /// `agent.checkpoints.turn_max_s`: a turn the host never reported as ended counts as ended
    /// after this long, so a host that died does not block a restore for good (1800 s).
    #[serde(default = "checkpoint_turn_max")]
    pub checkpoint_turn_max: Seconds,
    /// `companion.budget.*`.
    pub assembler: AssemblerBudget,
    /// `companion.idle_s` and `companion.side_close_s`.
    pub idle: IdleRules,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            start_mode: Mode::Auto,
            rows: RowSets::default(),
            budget: STANDARD_BUDGET,
            review: ReviewTimeouts::STANDARD,
            mass_at: Count(20),
            confirm_expiry: Seconds(120),
            confirm_proceed: proceed_within(),
            breaker: BreakerLimits {
                consecutive: Count(3),
                recent: Count(10),
                window: Count(50),
                probing: Count(3),
            },
            checkpoint_keep: checkpoint_keep(),
            checkpoint_days: checkpoint_days(),
            checkpoint_max_files: checkpoint_max_files(),
            checkpoint_wait: checkpoint_wait(),
            checkpoint_turn_max: checkpoint_turn_max(),
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

const fn w(key: &'static str, v: &'static str) -> SettingRow {
    SettingRow {
        key,
        default: SettingValue::Word(v),
    }
}

/// Every row, in the order the Intelligence page lists them. The retention row `companion.*`
/// (30 days) is almanac's `RuleSet::standard()`; the desktop-scope join rule is `prov`'s.
pub const SETTING_ROWS: &[SettingRow] = &[
    w("agent.start_mode", "auto"),
    w("agent.auto.inside", "runs"),
    w("agent.auto.other", "check"),
    w("agent.auto.send", "asks"),
    w("agent.auto.delete", "asks"),
    w("agent.auto.screen", "once"),
    w("agent.manual.inside", "asks"),
    w("agent.manual.other", "asks"),
    w("agent.manual.send", "asks"),
    w("agent.manual.delete", "asks"),
    w("agent.manual.screen", "each"),
    n("agent.mass_at", 20),
    n("agent.confirm.expiry_s", 120),
    n("agent.confirm.proceed_ms", 10_000),
    n("agent.breaker.consecutive", 3),
    n("agent.breaker.recent", 10),
    n("agent.checkpoints.keep", 20),
    n("agent.checkpoints.days", 14),
    n("agent.checkpoints.max_files", 50_000),
    n("agent.checkpoints.wait_s", 10),
    n("agent.checkpoints.turn_max_s", 1800),
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
            "agent.start_mode" => Some(SettingValue::Word(self.start_mode.word())),
            "agent.auto.inside" => Some(SettingValue::Word(self.rows.auto.inside.word())),
            "agent.auto.other" => Some(SettingValue::Word(self.rows.auto.other.word())),
            "agent.auto.send" => Some(SettingValue::Word(self.rows.auto.send.word())),
            "agent.auto.delete" => Some(SettingValue::Word(self.rows.auto.delete.word())),
            "agent.auto.screen" => Some(SettingValue::Word(self.rows.auto.screen.word())),
            "agent.manual.inside" => Some(SettingValue::Word(self.rows.manual.inside.word())),
            "agent.manual.other" => Some(SettingValue::Word(self.rows.manual.other.word())),
            "agent.manual.send" => Some(SettingValue::Word(self.rows.manual.send.word())),
            "agent.manual.delete" => Some(SettingValue::Word(self.rows.manual.delete.word())),
            "agent.manual.screen" => Some(SettingValue::Word(self.rows.manual.screen.word())),
            "agent.mass_at" => num(c(self.mass_at)),
            "agent.confirm.expiry_s" => num(i64::from(self.confirm_expiry.0)),
            "agent.confirm.proceed_ms" => num(i64::from(self.confirm_proceed.0)),
            "agent.breaker.consecutive" => num(c(self.breaker.consecutive)),
            "agent.breaker.recent" => num(c(self.breaker.recent)),
            "agent.checkpoints.keep" => num(c(self.checkpoint_keep)),
            "agent.checkpoints.days" => num(i64::from(self.checkpoint_days.0)),
            "agent.checkpoints.max_files" => num(c(self.checkpoint_max_files)),
            "agent.checkpoints.wait_s" => num(i64::from(self.checkpoint_wait.0)),
            "agent.checkpoints.turn_max_s" => num(i64::from(self.checkpoint_turn_max.0)),
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
}
