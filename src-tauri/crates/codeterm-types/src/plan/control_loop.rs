use serde::{Deserialize, Serialize};

pub const CURATOR_MIN_DELAY_SECS: u64 = 45 * 60;
pub const CURATOR_MAX_DELAY_SECS: u64 = 2 * 60 * 60;
pub const CURATOR_IDLE_FLOOR_MS: u64 = 4 * 60 * 60 * 1000;
pub const CURATOR_TIMEOUT_SECS: u64 = 15 * 60;
pub const CURATOR_TIMEOUT_MS: u64 = CURATOR_TIMEOUT_SECS * 1000;
pub const CURATOR_MAX_TURNS: u64 = 24;
pub const CURATOR_MAX_OUTPUT_TOKENS: u64 = 64_000;
pub const CURATOR_INITIAL_WINDOW_MS: u64 = CURATOR_MAX_DELAY_SECS * 1000;
pub const CURATOR_ELEVATED_LANES: usize = 5;
pub const CURATOR_HIGH_LANES: usize = 10;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum CuratorOutcome {
    Quiet,
    Nudged,
    #[default]
    Done,
    Partial,
    Failed,
    Fallback,
    ModeChanged,
}
impl std::fmt::Display for CuratorOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Quiet => "quiet",
            Self::Nudged => "nudged",
            Self::Done => "done",
            Self::Partial => "partial",
            Self::Failed => "failed",
            Self::Fallback => "fallback",
            Self::ModeChanged => "mode_changed",
        })
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum CuratorAsked {
    #[default]
    #[serde(rename = "")]
    Unrecorded,
    None,
    Answered,
    Skip,
    Timeout,
}
impl std::str::FromStr for CuratorAsked {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "none" => Ok(Self::None),
            "answered" => Ok(Self::Answered),
            "skip" => Ok(Self::Skip),
            "timeout" => Ok(Self::Timeout),
            _ => Err("invalid ASKED outcome".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum CuratorFailure {
    ProviderUnavailable,
    MirrorUnavailable,
    ProfileUnavailable,
    OrchestratorAbsent,
    OrchestratorUnbound,
    LaunchFailed,
    ConsecutiveFailures,
}
impl CuratorFailure {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProviderUnavailable => "provider_unavailable",
            Self::MirrorUnavailable => "mirror_unavailable",
            Self::ProfileUnavailable => "profile_unavailable",
            Self::OrchestratorAbsent => "orchestrator_absent",
            Self::OrchestratorUnbound => "orchestrator_unbound",
            Self::LaunchFailed => "launch_failed",
            Self::ConsecutiveFailures => "consecutive_failures",
        }
    }
}
impl From<CuratorFailure> for String {
    fn from(value: CuratorFailure) -> Self {
        value.as_str().into()
    }
}
impl std::fmt::Display for CuratorFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum CuratorSuspension {
    RemindersDisabled,
    Paused,
    Complete,
    WaitingOnHuman,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum ControlLoopMode {
    #[default]
    Curator,
    Heartbeat,
    Off,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, ts_rs::TS)]
pub struct CuratorCounts {
    pub rounds: u64,
    pub quiet: u64,
    pub nudged: u64,
    pub learnings: u64,
    pub saves: u64,
    pub fallbacks: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, ts_rs::TS)]
pub struct CuratorRound {
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub tab_id: Option<String>,
    #[serde(default)]
    pub synthesis: bool,
    #[serde(default)]
    pub move_line: String,
    #[serde(default)]
    pub retro: Vec<String>,
    #[serde(default)]
    pub learn: Vec<String>,
    #[serde(default)]
    pub next_checks: Vec<String>,
    #[serde(default)]
    pub ready: Option<String>,
    pub number: u64,
    pub started_at: u64,
    pub ended_at: u64,
    pub outcome: CuratorOutcome,
    pub summary: String,
    pub next_delay_secs: u64,
    pub reason: String,
    pub learnings: u64,
    pub save_id: Option<String>,
    pub asked: CuratorAsked,
    pub digest_hash: String,
    pub section_sizes: std::collections::BTreeMap<String, usize>,
    pub turns: u64,
    pub output_tokens: Option<u64>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, ts_rs::TS)]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, ts_rs::TS)]
pub struct CuratorRun {
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub synthesis: bool,
    #[serde(default)]
    pub synthesis_group_close_at: Option<u64>,
    pub number: u64,
    pub started_at: u64,
    pub tab_id: Option<String>,
    pub session_id: Option<String>,
    pub binding_nonce: Option<String>,
    pub flagged_blueprints: Vec<String>,
    pub grant: Option<CuratorGrant>,
    pub asked: bool,
    #[serde(default)]
    pub question_pending: bool,
    pub refused_actions: Vec<String>,
    pub digest_hash: String,
    pub section_sizes: std::collections::BTreeMap<String, usize>,
    pub turns: u64,
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub save_id: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum CuratorMessageReceipt {
    Accepted,
    NothingWritten,
    Uncertain,
}

impl CuratorRun {
    pub fn reserve_question(&mut self) -> bool {
        if self.asked || self.question_pending {
            return false;
        }
        self.question_pending = true;
        true
    }
    pub fn settle_question(&mut self, receipt: CuratorMessageReceipt) {
        match receipt {
            CuratorMessageReceipt::Accepted => {
                self.asked = true;
                self.question_pending = false;
            }
            CuratorMessageReceipt::NothingWritten => self.question_pending = false,
            CuratorMessageReceipt::Uncertain => {}
        }
    }
    pub fn refuse_action(&mut self, action: String) {
        self.refused_actions.push(action);
        let excess = self.refused_actions.len().saturating_sub(20);
        self.refused_actions.drain(..excess);
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, ts_rs::TS)]
pub struct CuratorAgentState {
    pub state: String,
    pub since: u64,
}

#[cfg(test)]
#[path = "control_loop_tests.rs"]
mod tests;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, ts_rs::TS)]
#[serde(default)]
pub struct ControlLoop {
    pub manual_run_requested: bool,
    pub real_rounds_started: u64,
    pub last_synthesis_group_close_at: Option<u64>,
    pub mode: ControlLoopMode,
    pub heartbeat_interval_secs: Option<u64>,
    pub next_at: Option<u64>,
    pub last_round: Option<CuratorRound>,
    pub fallback_reason: Option<CuratorFailure>,
    pub suspended_reason: Option<CuratorSuspension>,
    /// Projected from `curator_round` strides by readers; storage never supplies it.
    #[serde(skip_deserializing)]
    pub counts_7d: CuratorCounts,
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
    /// The Team's registered Curator schedule, clamped into the host bounds.
    pub curator_window: Option<CuratorWindow>,
}

/// The Curator's delay window in seconds; it never leaves the host bounds.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
pub struct CuratorWindow {
    pub min_secs: u64,
    pub max_secs: u64,
}

impl Default for CuratorWindow {
    fn default() -> Self {
        Self::HOST
    }
}

impl CuratorWindow {
    pub const HOST: Self = Self {
        min_secs: CURATOR_MIN_DELAY_SECS,
        max_secs: CURATOR_MAX_DELAY_SECS,
    };

    /// A Team's `<min>..<max>` with each end clamped into the host bounds:
    /// a narrower window narrows, a wider one clamps.
    pub fn within_host(min_secs: u64, max_secs: u64) -> Self {
        let clamp = |secs: u64| secs.clamp(CURATOR_MIN_DELAY_SECS, CURATOR_MAX_DELAY_SECS);
        Self {
            min_secs: clamp(min_secs.min(max_secs)),
            max_secs: clamp(max_secs.max(min_secs)),
        }
    }

    /// The chosen delay inside this window; none chosen is its minimum.
    pub fn delay(self, chosen: Option<u64>) -> u64 {
        // A stored window is re-clamped, so a hand-edited plan cannot escape the host bounds.
        let window = Self::within_host(self.min_secs, self.max_secs);
        chosen
            .unwrap_or(window.min_secs)
            .clamp(window.min_secs, window.max_secs)
    }
}

impl ControlLoop {
    /// The Team's Curator window, or the host window when the Team registers none.
    pub fn window(&self) -> CuratorWindow {
        self.curator_window.unwrap_or_default()
    }

    pub fn delay(&self, chosen: Option<u64>) -> u64 {
        self.window().delay(chosen)
    }

    pub fn defer(&mut self, now: u64, chosen: Option<u64>) {
        let delay = self.delay(chosen);
        self.next_at =
            (self.mode == ControlLoopMode::Curator).then(|| now.saturating_add(delay * 1000));
    }

    pub fn owner_heartbeat(&self) -> bool {
        self.mode != ControlLoopMode::Off
            && (self.mode == ControlLoopMode::Heartbeat || self.fallback_reason.is_some())
    }

    pub fn interval(&self, machine_default: u64) -> u64 {
        if self.mode == ControlLoopMode::Heartbeat {
            self.heartbeat_interval_secs.unwrap_or(machine_default)
        } else {
            machine_default
        }
    }

    /// Round history lives in `curator_round` strides; the header keeps only the latest.
    pub fn record(&mut self, round: CuratorRound) {
        self.last_round = Some(round);
    }
}
