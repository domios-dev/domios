use super::super::state::{mission_control, TaskCounts};
use crate::plan::{
    ControlLoopMode, CuratorCounts, CuratorFailure, CuratorRound, CuratorSuspension,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, ts_rs::TS)]
pub struct PlanBoardState {
    pub plan_id: String,
    pub revision: String,
    pub task_counts: TaskCounts,
    pub control_loop: PlanBoardControlLoop,
    pub mission_control: PlanBoardMissionControl,
    pub next_event_cursor: Option<usize>,
    pub event_window_loaded: bool,
}

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
pub struct PlanBoardControlLoop {
    pub mode: ControlLoopMode,
    pub heartbeat_interval_secs: Option<u64>,
    pub next_at: Option<u64>,
    pub last_round: Option<CuratorRound>,
    pub fallback_reason: Option<CuratorFailure>,
    pub manual_run_requested: bool,
    pub running: Option<PlanBoardCuratorRun>,
    #[serde(default)]
    pub counts_7d: CuratorCounts,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub suspended_reason: Option<Option<CuratorSuspension>>,
}

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
pub struct PlanBoardCuratorRun {
    pub number: u64,
}

#[derive(Debug, Serialize, ts_rs::TS)]
pub struct PlanBoardMissionControl {
    pub bounces: mission_control::BounceSummary,
    pub event_log: mission_control::EventLog,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub spend: Option<mission_control::MissionControlSpend>,
}
