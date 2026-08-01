//! Shared lane-liveness derivation — the single source of truth for how a
//! task's assigned tab maps to a live/idle/gone verdict. Consumed by reconcile,
//! plan-lanes, and the lane watchdog so their verdicts can never disagree.

use serde::Serialize;

use super::reconcile::NEEDS_REASSIGN_LABEL;

pub const DEFAULT_T_STALL_MS: u64 = 10 * 60 * 1000;
pub const DEFAULT_T_OVERDUE_MS: u64 = 45 * 60 * 1000;

pub const STALLED_LABEL: &str = "stalled";
pub const REPORT_OVERDUE_LABEL: &str = "report-overdue";
pub const PEER_OFFLINE_LABEL: &str = "peer-offline";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneThresholds {
    pub t_stall_ms: u64,
    pub t_overdue_ms: u64,
}

impl Default for LaneThresholds {
    fn default() -> Self {
        Self {
            t_stall_ms: DEFAULT_T_STALL_MS,
            t_overdue_ms: DEFAULT_T_OVERDUE_MS,
        }
    }
}

/// Inputs a caller gathers about one in_progress/in_review task's tab. The
/// watchdog never invents state for tab-less tasks (that stays UI-owned) — so
/// `has_assigned_tab == false` short-circuits to `NoTab`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaneInputs {
    pub has_assigned_tab: bool,
    pub tab_present_local: bool,
    pub has_peer: bool,
    /// Mesh presence for the recorded peer: Some(true)=online, Some(false)=offline,
    /// None=unknown/not-checked.
    pub peer_online: Option<bool>,
    /// Live agent-tracker verdict: Some(true)=Working, Some(false)=Idle/Waiting,
    /// None=unknown.
    pub tracker_active: Option<bool>,
    pub last_activity_ms: u64,
    pub now_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneLiveness {
    NoTab,
    Working,
    IdleStalled,
    ReportOverdue,
    TabGone,
    PeerOffline,
}

impl LaneInputs {
    fn activity_age(&self) -> u64 {
        self.now_ms.saturating_sub(self.last_activity_ms)
    }
}

/// Single derivation both reconcile and the watchdog share. Conservative by
/// design: peer-offline parks the verdict (a sleeping laptop must not look gone),
/// tab-gone only fires when the tab is absent locally AND no live peer could
/// still host it, and report-overdue is amber (long honest work), not gone.
pub fn derive_lane_liveness(inputs: &LaneInputs, thresholds: &LaneThresholds) -> LaneLiveness {
    if !inputs.has_assigned_tab {
        return LaneLiveness::NoTab;
    }
    if inputs.has_peer && inputs.peer_online == Some(false) {
        return LaneLiveness::PeerOffline;
    }
    if !inputs.tab_present_local {
        // Absent locally: gone only when no peer could still own it.
        return if inputs.has_peer {
            LaneLiveness::Working
        } else {
            LaneLiveness::TabGone
        };
    }
    let age = inputs.activity_age();
    if inputs.tracker_active == Some(false) && age > thresholds.t_stall_ms {
        return LaneLiveness::IdleStalled;
    }
    if inputs.tracker_active == Some(true) && age > thresholds.t_overdue_ms {
        return LaneLiveness::ReportOverdue;
    }
    LaneLiveness::Working
}

/// A watchdog verdict: what to flag, record, and whether to notify. NEVER
/// carries a task-status transition — the watchdog flags and records only;
/// status transitions stay orchestrator authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneVerdict {
    pub liveness: LaneLiveness,
    pub status_label: Option<&'static str>,
    pub stride: Option<String>,
    pub notify_orchestrator: bool,
}

/// Map a liveness verdict onto flag/stride/notify decisions. Tab-gone reuses
/// reconcile's `needs-reassign` label so the two surfaces agree.
pub fn verdict_for(liveness: LaneLiveness, inputs: &LaneInputs) -> LaneVerdict {
    let (status_label, notify): (Option<&'static str>, bool) = match liveness {
        LaneLiveness::NoTab | LaneLiveness::Working => (None, false),
        LaneLiveness::PeerOffline => (Some(PEER_OFFLINE_LABEL), false),
        LaneLiveness::IdleStalled => (Some(STALLED_LABEL), true),
        LaneLiveness::ReportOverdue => (Some(REPORT_OVERDUE_LABEL), true),
        LaneLiveness::TabGone => (Some(NEEDS_REASSIGN_LABEL), true),
    };
    let stride = status_label.map(|label| {
        format!(
            "lane-watchdog: {label} (activity {}ms ago, tracker={:?}, peer_online={:?})",
            inputs.activity_age(),
            inputs.tracker_active,
            inputs.peer_online
        )
    });
    LaneVerdict {
        liveness,
        status_label,
        stride,
        notify_orchestrator: notify,
    }
}

#[cfg(test)]
mod tests;
