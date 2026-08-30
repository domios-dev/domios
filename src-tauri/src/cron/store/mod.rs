//! Cron job store — types, persistence, and per-concern impl modules.
//!
//! Layout:
//!   - [`schedule`] — pure schedule parsing/validation/computation
//!   - [`crud`]     — CronStore CRUD + run history recording
//!   - [`tabs`]    — tab-ownership tracking + per-job log persistence
//!
//! Public surface: types + the `CronStore` constructors / save below, with the
//! schedule helpers re-exported so callers keep using `cron::store::compute_next_run`
//! (and friends) unchanged.

mod crud;
mod schedule;
mod tabs;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub use schedule::{
    compute_backoff, compute_next_run, compute_next_run_at, compute_next_run_in,
    compute_next_run_jittered, compute_next_run_jittered_at, compute_next_run_jittered_in,
    parse_oneshot_schedule, parse_oneshot_schedule_at, parse_tz, to_6field, validate_schedule,
    validate_tz, MAX_CONSECUTIVE_FAILURES,
};

use crate::cron::clock::{system_clock, SharedClock};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CronRunStatus {
    Running,
    Success,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronRunInfo {
    pub run_id: String,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub status: CronRunStatus,
    pub tab_id: Option<String>,
    // session_id + cwd captured before the hidden tab closes so the read-only chat-ui can resolve and render this past run's jsonl.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    pub schedule: String,
    pub enabled: bool,
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub task: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Mem skill slug (`codeterm mem skill new <slug>`) resolved and injected
    /// into the task brief at fire time — see `cron::scheduler::fire_job`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub creator_tab_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub report_target_tab_id: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub oneshot: bool,
    #[serde(default = "default_max_history")]
    pub max_history: usize,
    pub created_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run: Option<CronRunInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_run: Option<u64>,
    /// None → 10% of interval clamped to [1m, 30m]. Some(0) → off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jitter_max_ms: Option<u64>,
    /// IANA timezone (e.g. "Europe/Riga"). None → UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tz: Option<String>,
    /// Per-cron run timeout. None → fall back to global memory.worker_timeout_seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
    /// Auto-disables at >= 5.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub consecutive_failures: u32,
    /// Protected crons cannot be deleted, only toggled.
    #[serde(default, skip_serializing_if = "is_false")]
    pub protected: bool,
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

fn default_max_history() -> usize {
    100
}

fn is_false(v: &bool) -> bool {
    !v
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CronFile {
    version: String,
    jobs: Vec<CronJob>,
    history: HashMap<String, Vec<CronRunInfo>>,
}

impl Default for CronFile {
    fn default() -> Self {
        Self {
            version: "1".to_string(),
            jobs: Vec::new(),
            history: HashMap::new(),
        }
    }
}

pub struct CronStore {
    file: CronFile,
    file_path: PathBuf,
    tab_owners: HashMap<String, (String, String)>,
    clock: SharedClock,
}

impl CronStore {
    pub fn new() -> Self {
        Self::new_with_clock(system_clock())
    }

    /// Production path + an injected clock — the daemon uses this with a
    /// MockClock when CODETERM_TEST_CLOCK=1 (e2e time travel).
    pub fn new_with_clock(clock: SharedClock) -> Self {
        let dir = crate::paths::data_dir();
        Self::load(dir.join("crons.json"), clock)
    }

    #[cfg(test)]
    pub fn new_with_path(path: PathBuf) -> Self {
        Self::load(path, system_clock())
    }

    #[cfg(test)]
    pub fn new_with_path_and_clock(path: PathBuf, clock: SharedClock) -> Self {
        Self::load(path, clock)
    }

    fn load(file_path: PathBuf, clock: SharedClock) -> Self {
        let file = if file_path.exists() {
            match fs::read_to_string(&file_path) {
                Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
                Err(_) => CronFile::default(),
            }
        } else {
            CronFile::default()
        };
        Self {
            file,
            file_path,
            tab_owners: HashMap::new(),
            clock,
        }
    }

    /// Current time from the store's clock — the single time source for
    /// everything cron (scheduler ticks, run records, next-run math).
    pub fn now_ms(&self) -> u64 {
        self.clock.now_ms()
    }

    pub fn save(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&self.file).map_err(|e| e.to_string())?;
        fs::write(&self.file_path, json).map_err(|e| e.to_string())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CronCreateParams {
    pub name: String,
    pub workspace_id: String,
    #[serde(default)]
    pub team_id: Option<String>,
    #[serde(default)]
    pub detach_from_team: bool,
    pub schedule: String,
    pub task: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub skill: Option<String>,
    #[serde(default)]
    pub creator_tab_id: Option<String>,
    #[serde(default)]
    pub report_target_tab_id: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub max_history: Option<usize>,
    #[serde(default)]
    pub oneshot: Option<bool>,
    #[serde(default)]
    pub jitter_max_ms: Option<u64>,
    #[serde(default)]
    pub tz: Option<String>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CronUpdateParams {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub schedule: Option<String>,
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    /// `Some("")` clears the skill back to none. `Some(non-empty)` sets it. `None` leaves unchanged.
    #[serde(default)]
    pub skill: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub max_history: Option<usize>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub jitter_max_ms: Option<u64>,
    /// `Some("")` clears tz back to UTC. `Some(non-empty)` sets it. `None` leaves unchanged.
    #[serde(default)]
    pub tz: Option<String>,
    /// `Some(0)` clears the override (back to global). `Some(non-zero)` sets it. `None` leaves unchanged.
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}
