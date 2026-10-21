use serde::{Deserialize, Serialize};

pub const CURATOR_MIN_DELAY_SECS: u64 = 45 * 60;
pub const CURATOR_MAX_DELAY_SECS: u64 = 2 * 60 * 60;
pub const CURATOR_IDLE_FLOOR_MS: u64 = 4 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ControlLoopMode {
    #[default]
    Curator,
    Heartbeat,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CuratorCounts {
    pub rounds: u64,
    pub quiet: u64,
    pub nudged: u64,
    pub learnings: u64,
    pub saves: u64,
    pub fallbacks: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CuratorRound {
    pub number: u64,
    pub started_at: u64,
    pub ended_at: u64,
    pub outcome: String,
    pub summary: String,
    pub next_delay_secs: u64,
    pub reason: String,
    pub learnings: u64,
    pub save_id: Option<String>,
    pub asked: String,
    pub digest_hash: String,
    pub section_sizes: std::collections::BTreeMap<String, usize>,
    pub turns: u64,
    pub output_tokens: Option<u64>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CuratorGrant {
    pub tab_id: String,
    pub session_id: String,
    pub binding_nonce: String,
    pub round: u64,
    pub blueprints: Vec<String>,
    pub plan_skills: Vec<String>,
    #[serde(default)]
    pub save_candidates: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CuratorRun {
    pub number: u64,
    pub started_at: u64,
    pub tab_id: Option<String>,
    pub session_id: Option<String>,
    pub binding_nonce: Option<String>,
    pub flagged_blueprints: Vec<String>,
    pub grant: Option<CuratorGrant>,
    pub asked: bool,
    pub refused_actions: Vec<String>,
    pub digest_hash: String,
    pub section_sizes: std::collections::BTreeMap<String, usize>,
    pub turns: u64,
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub save_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CuratorAgentState {
    pub state: String,
    pub since: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ControlLoop {
    pub mode: ControlLoopMode,
    pub heartbeat_interval_secs: Option<u64>,
    pub next_at: Option<u64>,
    pub last_round: Option<CuratorRound>,
    pub fallback_reason: Option<String>,
    pub suspended_reason: Option<String>,
    pub counts_7d: CuratorCounts,
    pub rounds: Vec<CuratorRound>,
    pub consecutive_failures: u32,
    pub last_real_round_at: Option<u64>,
    pub cron_id: Option<String>,
    pub running: Option<CuratorRun>,
    pub owner_turn_cursor: Option<String>,
    pub owner_session_key: Option<String>,
    pub task_states: std::collections::BTreeMap<String, String>,
    pub agent_states: std::collections::BTreeMap<String, CuratorAgentState>,
    pub observed_owner_answers: std::collections::BTreeSet<String>,
    pub save_candidates: std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
    pub save_candidate_shapes: std::collections::BTreeMap<String, String>,
}

pub fn curator_delay(chosen: Option<u64>) -> u64 {
    chosen
        .unwrap_or(CURATOR_MIN_DELAY_SECS)
        .clamp(CURATOR_MIN_DELAY_SECS, CURATOR_MAX_DELAY_SECS)
}

impl ControlLoop {
    pub fn owner_heartbeat(&self) -> bool {
        self.mode == ControlLoopMode::Heartbeat || self.fallback_reason.is_some()
    }

    pub fn interval(&self, machine_default: u64) -> u64 {
        if self.mode == ControlLoopMode::Heartbeat {
            self.heartbeat_interval_secs.unwrap_or(machine_default)
        } else {
            machine_default
        }
    }

    pub fn record(&mut self, round: CuratorRound) {
        let now = round.ended_at;
        self.rounds.push(round.clone());
        self.last_round = Some(round);
        self.refresh_counts(now);
    }

    pub fn refresh_counts(&mut self, now: u64) {
        let cutoff = now.saturating_sub(7 * 24 * 60 * 60 * 1000);
        self.rounds.retain(|r| r.ended_at >= cutoff);
        self.counts_7d = CuratorCounts::default();
        for r in &self.rounds {
            match r.outcome.as_str() {
                "quiet" => self.counts_7d.quiet += 1,
                "nudged" => self.counts_7d.nudged += 1,
                "fallback" => self.counts_7d.fallbacks += 1,
                _ => self.counts_7d.rounds += 1,
            }
            self.counts_7d.learnings += r.learnings;
            self.counts_7d.saves += u64::from(r.save_id.is_some());
        }
    }
}
