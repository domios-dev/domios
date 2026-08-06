//! Per-pane session-restore report. Session restore used to drop worker panes
//! silently; this records, for every agent/worker pane the snapshot held,
//! whether it came back — so a dropped pane is surfaced, never lost.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// One agent/worker pane the dead session snapshot carried into restore.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RestorePane {
    pub tab_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DropReason {
    /// Present in the snapshot, absent from the restored multiplexer state
    /// (ghost-purged, authority-stripped, or failed to rebuild).
    NotRestored,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaneOutcome {
    #[serde(flatten)]
    pub pane: RestorePane,
    pub restored: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drop_reason: Option<DropReason>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RestoreReport {
    pub bound: u32,
    pub restored: u32,
    pub dropped: u32,
    pub panes: Vec<PaneOutcome>,
}

impl RestoreReport {
    pub fn dropped_panes(&self) -> impl Iterator<Item = &PaneOutcome> {
        self.panes.iter().filter(|p| !p.restored)
    }

    pub fn has_dropped(&self) -> bool {
        self.dropped > 0
    }
}

/// Deterministic: outcome depends only on which bound tab ids came back, and
/// panes are ordered by tab id so the report is stable across runs.
pub fn build_restore_report(bound: &[RestorePane], restored_ids: &BTreeSet<String>) -> RestoreReport {
    let mut panes: Vec<PaneOutcome> = bound
        .iter()
        .map(|pane| {
            let restored = restored_ids.contains(&pane.tab_id);
            PaneOutcome {
                pane: pane.clone(),
                restored,
                drop_reason: (!restored).then_some(DropReason::NotRestored),
            }
        })
        .collect();
    panes.sort_by(|a, b| a.pane.tab_id.cmp(&b.pane.tab_id));
    let restored = panes.iter().filter(|p| p.restored).count() as u32;
    RestoreReport {
        bound: panes.len() as u32,
        restored,
        dropped: panes.len() as u32 - restored,
        panes,
    }
}

/// Surface the report: WARN per dropped pane (named), INFO summary. A dropped
/// pane leaves an explicit log line instead of a silently-shorter workspace.
pub fn log_report(report: &RestoreReport) {
    if report.bound == 0 {
        return;
    }
    if report.has_dropped() {
        log::warn!(
            "[restore] {} of {} agent pane(s) not restored — dropped panes follow",
            report.dropped,
            report.bound
        );
        for outcome in report.dropped_panes() {
            log::warn!(
                "[restore] dropped pane tab={} role={:?} agent_type={:?} session={:?}",
                outcome.pane.tab_id,
                outcome.pane.role,
                outcome.pane.agent_type,
                outcome.pane.agent_session_id
            );
        }
    } else {
        log::info!(
            "[restore] all {} agent pane(s) restored",
            report.bound
        );
    }
}

fn report_path() -> PathBuf {
    crate::paths::data_dir()
        .join("state")
        .join("recovery")
        .join("last_restore_report.json")
}

/// Persist durably so the report survives the process and an operator (or the
/// CLI) can read what restore dropped after the fact.
pub fn persist(report: &RestoreReport) {
    let path = report_path();
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            log::error!("[restore] create {}: {}", parent.display(), error);
            return;
        }
    }
    match serde_json::to_string_pretty(report) {
        Ok(raw) => {
            if let Err(error) = std::fs::write(&path, raw) {
                log::error!("[restore] write {}: {}", path.display(), error);
            }
        }
        Err(error) => log::error!("[restore] serialize report: {}", error),
    }
}

/// Compute, surface, and persist in one call. Returns the report for callers
/// that also feed binding evidence to the recovery record.
pub fn record(bound: &[RestorePane], restored_ids: &BTreeSet<String>) -> RestoreReport {
    let report = build_restore_report(bound, restored_ids);
    log_report(&report);
    persist(&report);
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(id: &str, role: Option<&str>, agent: Option<&str>) -> RestorePane {
        RestorePane {
            tab_id: id.to_string(),
            role: role.map(str::to_string),
            agent_type: agent.map(str::to_string),
            agent_session_id: Some(format!("session-{id}")),
        }
    }

    fn ids(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn dropped_worker_pane_is_surfaced_not_lost() {
        // Orchestrator + newest worker restored; three older workers dropped —
        // the exact scenario from the report. Each must appear as dropped.
        let bound = vec![
            pane("orch", Some("orchestrator"), Some("claude")),
            pane("w-new", Some("worker"), Some("codex")),
            pane("w-1", Some("worker"), Some("codex")),
            pane("w-2", Some("worker"), Some("codex")),
            pane("w-3", Some("worker"), Some("codex")),
        ];
        let report = build_restore_report(&bound, &ids(&["orch", "w-new"]));
        assert_eq!(report.bound, 5);
        assert_eq!(report.restored, 2);
        assert_eq!(report.dropped, 3);
        let dropped: Vec<&str> = report
            .dropped_panes()
            .map(|o| o.pane.tab_id.as_str())
            .collect();
        assert_eq!(dropped, vec!["w-1", "w-2", "w-3"]);
        assert!(report
            .dropped_panes()
            .all(|o| o.drop_reason == Some(DropReason::NotRestored)));
    }

    #[test]
    fn full_restore_reports_no_drop() {
        let bound = vec![
            pane("orch", Some("orchestrator"), Some("claude")),
            pane("w-1", Some("worker"), Some("codex")),
        ];
        let report = build_restore_report(&bound, &ids(&["orch", "w-1"]));
        assert!(!report.has_dropped());
        assert_eq!(report.restored, 2);
        assert_eq!(report.dropped_panes().count(), 0);
    }

    #[test]
    fn report_is_deterministic_regardless_of_bound_order() {
        let a = vec![pane("w-2", None, Some("codex")), pane("w-1", None, Some("codex"))];
        let b = vec![pane("w-1", None, Some("codex")), pane("w-2", None, Some("codex"))];
        let restored = ids(&["w-1"]);
        assert_eq!(
            build_restore_report(&a, &restored),
            build_restore_report(&b, &restored),
            "pane ordering must not change the report"
        );
    }

    #[test]
    fn empty_binding_set_is_a_clean_no_op_report() {
        let report = build_restore_report(&[], &ids(&[]));
        assert_eq!(report, RestoreReport::default());
        assert!(!report.has_dropped());
    }

    #[test]
    fn report_round_trips_through_json() {
        let bound = vec![pane("w-1", Some("worker"), Some("codex"))];
        let report = build_restore_report(&bound, &ids(&[]));
        let json = serde_json::to_string(&report).unwrap();
        let back: RestoreReport = serde_json::from_str(&json).unwrap();
        assert_eq!(report, back);
    }
}
