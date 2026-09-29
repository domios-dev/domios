//! Per-pane session-restore report. A pane counts as restored only when an
//! agent process was observed live behind it — the tab record coming back is
//! the question, never the answer. A pane whose record returned as a bare shell
//! is reported as NOT restored, and the all-N-restored summary is withheld.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use codeterm_types::pty::AgentLiveness;
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
    /// Newest lineage session of a team member whose provider exited before shutdown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exited_session_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DropReason {
    /// Present in the snapshot, absent from the restored multiplexer state
    /// (ghost-purged or failed to rebuild).
    NotRestored,
    /// The tab record came back, but no agent process is live behind it — the
    /// pane is a bare shell wearing its old identity.
    AgentNotLive,
    /// The provider had exited to its shell before shutdown, so restore relaunched
    /// nothing; `agent restart` resumes `exited_session_id` in place.
    AgentExitedBeforeShutdown,
    /// The tab record came back and liveness has not been observed yet. An
    /// honest third state: this pane is neither claimed restored nor lost.
    AgentLivenessUnobserved,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaneOutcome {
    #[serde(flatten)]
    pub pane: RestorePane,
    pub restored: bool,
    pub liveness: AgentLiveness,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drop_reason: Option<DropReason>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RestoreReport {
    pub bound: u32,
    pub restored: u32,
    pub dropped: u32,
    /// Records that came back without a live agent behind them.
    pub shell_only: u32,
    /// Records that came back with liveness not yet observable.
    pub unobserved: u32,
    /// How the previous epoch ended, when the recovery record knows: a clean
    /// shutdown reason, a crash classification, or nothing. Restore attributing
    /// its own losses is what turns the next incident into evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_epoch_end: Option<String>,
    /// Why restored agents were materialized by the backend alone, when they
    /// were: an unobserved wave then names what it is still waiting on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_materialization: Option<String>,
    pub panes: Vec<PaneOutcome>,
}

impl Default for RestoreReport {
    fn default() -> Self {
        Self {
            bound: 0,
            restored: 0,
            dropped: 0,
            shell_only: 0,
            unobserved: 0,
            previous_epoch_end: None,
            backend_materialization: None,
            panes: Vec::new(),
        }
    }
}

impl RestoreReport {
    #[cfg(test)]
    pub fn dropped_panes(&self) -> impl Iterator<Item = &PaneOutcome> {
        self.panes.iter().filter(|p| !p.restored)
    }

    /// True only when every bound pane came back with an agent observed live.
    #[cfg(test)]
    pub fn every_pane_has_a_live_agent(&self) -> bool {
        self.bound > 0 && self.restored == self.bound
    }

    pub fn with_previous_epoch_end(mut self, end: Option<String>) -> Self {
        self.previous_epoch_end = end;
        self
    }

    pub fn with_backend_materialization(mut self, note: Option<String>) -> Self {
        self.backend_materialization = note;
        self
    }
}

fn outcome_for(pane: &RestorePane, present: bool, liveness: AgentLiveness) -> PaneOutcome {
    // Structural absence dominates: a record that never came back cannot have a
    // liveness verdict at all.
    let (restored, drop_reason) = match (present, liveness) {
        (false, _) => (false, Some(DropReason::NotRestored)),
        (true, AgentLiveness::Live) => (true, None),
        (true, AgentLiveness::Absent) if pane.exited_session_id.is_some() => {
            (false, Some(DropReason::AgentExitedBeforeShutdown))
        }
        (true, AgentLiveness::Absent) => (false, Some(DropReason::AgentNotLive)),
        (true, AgentLiveness::Unobserved) => (false, Some(DropReason::AgentLivenessUnobserved)),
    };
    PaneOutcome {
        pane: pane.clone(),
        restored,
        liveness: if present {
            liveness
        } else {
            AgentLiveness::Absent
        },
        drop_reason,
    }
}

/// Deterministic: outcome depends only on which bound tab ids came back and
/// what was observed behind them; panes are ordered by tab id so the report is
/// stable across runs. A tab id present in `restored_ids` but missing from
/// `liveness` is `Unobserved`, never assumed live.
pub fn build_restore_report(
    bound: &[RestorePane],
    restored_ids: &BTreeSet<String>,
    liveness: &BTreeMap<String, AgentLiveness>,
) -> RestoreReport {
    let mut panes: Vec<PaneOutcome> = bound
        .iter()
        .map(|pane| {
            let present = restored_ids.contains(&pane.tab_id);
            let observed = liveness
                .get(&pane.tab_id)
                .copied()
                .unwrap_or(AgentLiveness::Unobserved);
            outcome_for(pane, present, observed)
        })
        .collect();
    panes.sort_by(|a, b| a.pane.tab_id.cmp(&b.pane.tab_id));
    let restored = panes.iter().filter(|p| p.restored).count() as u32;
    let shell_only = panes
        .iter()
        .filter(|p| {
            matches!(
                p.drop_reason,
                Some(DropReason::AgentNotLive | DropReason::AgentExitedBeforeShutdown)
            )
        })
        .count() as u32;
    let unobserved = panes
        .iter()
        .filter(|p| p.drop_reason == Some(DropReason::AgentLivenessUnobserved))
        .count() as u32;
    RestoreReport {
        bound: panes.len() as u32,
        restored,
        dropped: panes.len() as u32 - restored,
        shell_only,
        unobserved,
        previous_epoch_end: None,
        backend_materialization: None,
        panes,
    }
}

/// Select pane lines that this restore wave has not logged yet.
fn unlogged_pane_ids(report: &RestoreReport, logged: &mut BTreeSet<String>) -> Vec<String> {
    report
        .panes
        .iter()
        .filter_map(|outcome| {
            logged
                .insert(outcome.pane.tab_id.clone())
                .then(|| outcome.pane.tab_id.clone())
        })
        .collect()
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

fn build_record(
    bound: &[RestorePane],
    restored_ids: &BTreeSet<String>,
    liveness: &BTreeMap<String, AgentLiveness>,
    previous_epoch_end: Option<String>,
    backend_materialization: Option<String>,
) -> RestoreReport {
    let report = build_restore_report(bound, restored_ids, liveness)
        .with_previous_epoch_end(previous_epoch_end)
        .with_backend_materialization(backend_materialization);
    report
}

/// How the epoch that owned these panes ended, as one label. Restore losses
/// that are explained by a planned exit must not read as an unexplained crash,
/// and an unexplained one must not hide behind a plausible story.
pub fn previous_epoch_end_label() -> Option<String> {
    let state = crate::crash::lifecycle::recovery_state();
    if let Some(crash) = state.last_crash.as_ref() {
        return Some(format!("crash:{:?}", crash.classification));
    }
    state
        .last_clean_shutdown
        .as_ref()
        .map(|clean| format!("clean_shutdown:{:?}", clean.reason))
}

/// The restore wave is retained while any pane may still reconcile to live.
/// Nothing here is time-driven: reports change only when lifecycle evidence does.
struct PendingRestore {
    bound: Vec<RestorePane>,
    previous_epoch_end: Option<String>,
    backend_materialization: Option<String>,
    observed: BTreeMap<String, AgentLiveness>,
    restored_ids: BTreeSet<String>,
    logged_panes: BTreeSet<String>,
    revision: u64,
}

static PENDING: parking_lot::Mutex<Option<PendingRestore>> = parking_lot::Mutex::new(None);
static REPORT_WRITE: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

/// Tab ids still eligible for lifecycle reconciliation. Empty when no wave is
/// armed, which is the cheap early-out for the settle hook.
pub fn pending_tab_ids() -> Vec<String> {
    PENDING
        .lock()
        .as_ref()
        .map(|pending| {
            pending
                .bound
                .iter()
                .map(|pane| pane.tab_id.clone())
                .collect()
        })
        .unwrap_or_default()
}

fn log_pending_report(report: &RestoreReport, pending: &mut PendingRestore) {
    if report.bound == 0 {
        return;
    }
    let fresh = unlogged_pane_ids(report, &mut pending.logged_panes);
    for tab_id in fresh {
        if let Some(outcome) = report.panes.iter().find(|pane| pane.pane.tab_id == tab_id) {
            let level = if outcome.restored {
                log::Level::Info
            } else {
                log::Level::Warn
            };
            let recovery = outcome
                .pane
                .exited_session_id
                .as_ref()
                .map(|session| {
                    format!(
                        " recover=codeterm agent restart --tab {} exited_session={}",
                        outcome.pane.tab_id, session
                    )
                })
                .unwrap_or_default();
            log::log!(
                level,
                "[restore] pane tab={} role={:?} agent_type={:?} session={:?} liveness={} restored={} reason={:?}{}",
                outcome.pane.tab_id,
                outcome.pane.role,
                outcome.pane.agent_type,
                outcome.pane.agent_session_id,
                outcome.liveness.as_str(),
                outcome.restored,
                outcome.drop_reason,
                recovery
            );
        }
    }
}

fn publish_pending_report(report: &RestoreReport, revision: u64) -> bool {
    let _writer = REPORT_WRITE.lock();
    let mut guard = PENDING.lock();
    let Some(pending) = guard.as_mut() else {
        return false;
    };
    if pending.revision != revision {
        return false;
    }
    persist(report);
    log_pending_report(report, pending);
    if report.dropped == 0 {
        *guard = None;
    }
    true
}

fn update_pending_observations(
    pending: &mut PendingRestore,
    restored_ids: &BTreeSet<String>,
    observed: &BTreeMap<String, AgentLiveness>,
) -> bool {
    if pending.restored_ids == *restored_ids && pending.observed == *observed {
        return false;
    }
    pending.restored_ids = restored_ids.clone();
    pending.observed = observed.clone();
    pending.revision = pending.revision.wrapping_add(1);
    true
}

/// Arm the wave and write the honest opening report: at this point the tab
/// records are back and not one agent has been observed.
pub fn arm(bound: Vec<RestorePane>, restored_ids: &BTreeSet<String>) -> RestoreReport {
    let previous_epoch_end = previous_epoch_end_label();
    let observed: BTreeMap<String, AgentLiveness> = BTreeMap::new();
    let report = build_record(
        &bound,
        restored_ids,
        &observed,
        previous_epoch_end.clone(),
        None,
    );
    *PENDING.lock() = Some(PendingRestore {
        bound,
        previous_epoch_end,
        backend_materialization: None,
        observed,
        restored_ids: restored_ids.clone(),
        logged_panes: BTreeSet::new(),
        revision: 1,
    });
    publish_pending_report(&report, 1);
    report
}

/// Name why the armed wave is still unobserved: restored agents went to the
/// backend without a frontend. Re-records at once; a no-op once the wave settled.
pub fn note_backend_materialization(note: &str, restored_ids: &BTreeSet<String>) -> bool {
    let Some((bound, previous_epoch_end, observed, revision)) = ({
        let mut guard = PENDING.lock();
        let Some(pending) = guard.as_mut() else {
            return false;
        };
        if pending.backend_materialization.as_deref() == Some(note) {
            return false;
        }
        pending.backend_materialization = Some(note.to_string());
        pending.revision = pending.revision.wrapping_add(1);
        Some((
            pending.bound.clone(),
            pending.previous_epoch_end.clone(),
            pending.observed.clone(),
            pending.revision,
        ))
    }) else {
        return false;
    };
    let report = build_record(
        &bound,
        restored_ids,
        &observed,
        previous_epoch_end,
        Some(note.to_string()),
    );
    publish_pending_report(&report, revision)
}

/// Fold fresh observations into the wave and retain dropped panes for correction.
pub fn settle(restored_ids: &BTreeSet<String>, observed: BTreeMap<String, AgentLiveness>) -> bool {
    let Some((bound, previous_epoch_end, backend_materialization, revision)) = ({
        let mut guard = PENDING.lock();
        let Some(pending) = guard.as_mut() else {
            return false;
        };
        if !update_pending_observations(pending, restored_ids, &observed) {
            return false;
        }
        Some((
            pending.bound.clone(),
            pending.previous_epoch_end.clone(),
            pending.backend_materialization.clone(),
            pending.revision,
        ))
    }) else {
        return false;
    };
    let report = build_record(
        &bound,
        restored_ids,
        &observed,
        previous_epoch_end,
        backend_materialization,
    );
    publish_pending_report(&report, revision)
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
            exited_session_id: None,
        }
    }

    fn ids(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn live(items: &[&str]) -> BTreeMap<String, AgentLiveness> {
        items
            .iter()
            .map(|s| (s.to_string(), AgentLiveness::Live))
            .collect()
    }

    fn observed(items: &[(&str, AgentLiveness)]) -> BTreeMap<String, AgentLiveness> {
        items
            .iter()
            .map(|(id, liveness)| (id.to_string(), *liveness))
            .collect()
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
        let report =
            build_restore_report(&bound, &ids(&["orch", "w-new"]), &live(&["orch", "w-new"]));
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
        let report = build_restore_report(&bound, &ids(&["orch", "w-1"]), &live(&["orch", "w-1"]));
        assert_eq!(report.dropped, 0);
        assert_eq!(report.restored, 2);
        assert_eq!(report.dropped_panes().count(), 0);
    }

    #[test]
    fn restore_report_reconciles_late_live() {
        let bound = vec![pane("opencode", Some("worker"), Some("provider"))];
        let restored_ids = ids(&["opencode"]);
        let absent = observed(&[("opencode", AgentLiveness::Absent)]);
        let mut pending = PendingRestore {
            bound: bound.clone(),
            previous_epoch_end: None,
            backend_materialization: None,
            observed: BTreeMap::new(),
            restored_ids: restored_ids.clone(),
            logged_panes: BTreeSet::new(),
            revision: 0,
        };

        assert!(update_pending_observations(&mut pending, &restored_ids, &absent));
        let first = build_restore_report(&pending.bound, &pending.restored_ids, &pending.observed);
        assert_eq!(first.dropped, 1);
        assert_eq!(unlogged_pane_ids(&first, &mut pending.logged_panes), vec!["opencode"]);

        let live = observed(&[("opencode", AgentLiveness::Live)]);
        assert!(update_pending_observations(&mut pending, &restored_ids, &live));
        let reconciled = build_restore_report(&pending.bound, &pending.restored_ids, &pending.observed);
        assert_eq!(reconciled.dropped, 0);
        assert_eq!(reconciled.restored, 1);
        assert!(unlogged_pane_ids(&reconciled, &mut pending.logged_panes).is_empty());
    }

    #[test]
    fn restore_report_logs_each_pane_once_across_revisions() {
        let pane = pane("pane-1", Some("worker"), Some("provider"));
        let bound = vec![pane];
        let restored_ids = ids(&["pane-1"]);
        let mut logged = BTreeSet::new();
        let absent = build_restore_report(
            &bound,
            &restored_ids,
            &observed(&[("pane-1", AgentLiveness::Absent)]),
        );
        assert_eq!(unlogged_pane_ids(&absent, &mut logged), vec!["pane-1"]);

        let live_report = build_restore_report(&bound, &restored_ids, &live(&["pane-1"]));
        assert!(unlogged_pane_ids(&live_report, &mut logged).is_empty());
    }

    #[test]
    fn report_is_deterministic_regardless_of_bound_order() {
        let a = vec![
            pane("w-2", None, Some("codex")),
            pane("w-1", None, Some("codex")),
        ];
        let b = vec![
            pane("w-1", None, Some("codex")),
            pane("w-2", None, Some("codex")),
        ];
        let restored = ids(&["w-1"]);
        let liveness = live(&["w-1"]);
        assert_eq!(
            build_restore_report(&a, &restored, &liveness),
            build_restore_report(&b, &restored, &liveness),
            "pane ordering must not change the report"
        );
    }

    #[test]
    fn empty_binding_set_is_a_clean_no_op_report() {
        let report = build_restore_report(&[], &ids(&[]), &live(&[]));
        assert_eq!(report, RestoreReport::default());
        assert_eq!(report.dropped, 0);
    }

    #[test]
    fn report_round_trips_through_json() {
        let bound = vec![pane("w-1", Some("worker"), Some("codex"))];
        let report = build_restore_report(&bound, &ids(&[]), &live(&[]));
        let json = serde_json::to_string(&report).unwrap();
        let back: RestoreReport = serde_json::from_str(&json).unwrap();
        assert_eq!(report, back);
    }

    /// THE acceptance target, from the live reproduction: the pane's tab id IS
    /// in the restored set, its role is intact, and there is no agent process.
    #[test]
    fn a_record_that_came_back_without_a_live_agent_is_not_restored() {
        let bound = vec![
            pane("orch", Some("orchestrator"), Some("claude")),
            pane("planner", Some("manager"), Some("claude")),
        ];
        let report = build_restore_report(
            &bound,
            &ids(&["orch", "planner"]),
            &observed(&[
                ("orch", AgentLiveness::Live),
                ("planner", AgentLiveness::Absent),
            ]),
        );

        assert_eq!(report.bound, 2);
        assert_eq!(report.restored, 1, "only the pane with a live agent counts");
        assert_eq!(report.dropped, 1);
        assert_eq!(report.shell_only, 1);

        let dropped: Vec<&PaneOutcome> = report.dropped_panes().collect();
        assert_eq!(dropped.len(), 1);
        assert_eq!(dropped[0].pane.tab_id, "planner");
        assert_eq!(dropped[0].pane.role.as_deref(), Some("manager"));
        assert_eq!(
            dropped[0].drop_reason,
            Some(DropReason::AgentNotLive),
            "a bare-shell pane must be distinguishable from one that never came back"
        );
        assert_ne!(dropped[0].drop_reason, Some(DropReason::NotRestored));
    }

    #[test]
    fn unobserved_liveness_is_neither_restored_nor_a_lost_record() {
        let bound = vec![pane("w-1", Some("worker"), Some("codex"))];
        let report = build_restore_report(
            &bound,
            &ids(&["w-1"]),
            &observed(&[("w-1", AgentLiveness::Unobserved)]),
        );
        assert_eq!(report.restored, 0);
        assert_eq!(report.unobserved, 1);
        assert_eq!(report.shell_only, 0);
        assert_eq!(
            report.panes[0].drop_reason,
            Some(DropReason::AgentLivenessUnobserved)
        );
    }

    #[test]
    fn a_missing_liveness_entry_is_unobserved_never_assumed_live() {
        let bound = vec![pane("w-1", Some("worker"), Some("codex"))];
        let report = build_restore_report(&bound, &ids(&["w-1"]), &BTreeMap::new());
        assert_eq!(report.restored, 0);
        assert_eq!(report.panes[0].liveness, AgentLiveness::Unobserved);
    }

    #[test]
    fn a_record_that_never_came_back_keeps_its_own_drop_reason() {
        let bound = vec![pane("gone", Some("worker"), Some("codex"))];
        let report = build_restore_report(
            &bound,
            &ids(&[]),
            &observed(&[("gone", AgentLiveness::Live)]),
        );
        assert_eq!(
            report.panes[0].drop_reason,
            Some(DropReason::NotRestored),
            "structural absence dominates a stale liveness entry"
        );
    }

    /// The success line is a claim about processes, so it is emitted only when
    /// every pane has one. Asserted on the gate the logger reads, never on the
    /// wording of the line.
    #[test]
    fn the_all_restored_summary_is_withheld_when_any_pane_has_no_live_agent() {
        let bound = vec![
            pane("orch", Some("orchestrator"), Some("claude")),
            pane("planner", Some("manager"), Some("claude")),
        ];

        let all_live = build_restore_report(
            &bound,
            &ids(&["orch", "planner"]),
            &live(&["orch", "planner"]),
        );
        assert!(all_live.every_pane_has_a_live_agent());

        for degraded in [AgentLiveness::Absent, AgentLiveness::Unobserved] {
            let report = build_restore_report(
                &bound,
                &ids(&["orch", "planner"]),
                &observed(&[("orch", AgentLiveness::Live), ("planner", degraded)]),
            );
            assert!(
                !report.every_pane_has_a_live_agent(),
                "summary must be withheld for {degraded:?}"
            );
            assert!(report.dropped > 0);
        }
    }

    #[test]
    fn the_report_carries_how_the_previous_epoch_ended() {
        let bound = vec![pane("orch", Some("orchestrator"), Some("claude"))];
        let report = build_restore_report(
            &bound,
            &ids(&["orch"]),
            &observed(&[("orch", AgentLiveness::Absent)]),
        )
        .with_previous_epoch_end(Some("clean_shutdown:update".to_string()));
        assert_eq!(
            report.previous_epoch_end.as_deref(),
            Some("clean_shutdown:update")
        );
        let back: RestoreReport =
            serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
        assert_eq!(back.previous_epoch_end, report.previous_epoch_end);
    }

    #[test]
    fn restore_contract_exited_member_pane_names_its_recoverable_session() {
        let exited = RestorePane {
            agent_type: None,
            agent_session_id: None,
            exited_session_id: Some("exited-session".into()),
            ..pane("worker", Some("worker"), None)
        };
        let bare = RestorePane {
            agent_session_id: None,
            ..pane("shell", Some("worker"), None)
        };
        let report = build_restore_report(
            &[exited, bare],
            &ids(&["worker", "shell"]),
            &observed(&[
                ("worker", AgentLiveness::Absent),
                ("shell", AgentLiveness::Absent),
            ]),
        );

        assert_eq!(report.restored, 0);
        assert_eq!(report.shell_only, 2);
        let reasons: Vec<_> = report
            .panes
            .iter()
            .map(|p| (p.pane.tab_id.as_str(), p.drop_reason))
            .collect();
        assert_eq!(
            reasons,
            vec![
                ("shell", Some(DropReason::AgentNotLive)),
                ("worker", Some(DropReason::AgentExitedBeforeShutdown)),
            ]
        );
        let back: RestoreReport =
            serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
        assert_eq!(back, report);
        assert_eq!(
            back.panes[1].pane.exited_session_id.as_deref(),
            Some("exited-session")
        );
    }

    #[test]
    fn restore_contract_unobserved_wave_names_backend_materialization_without_frontend() {
        let bound = vec![pane("worker", Some("worker"), Some("codex"))];
        let report = build_restore_report(&bound, &ids(&["worker"]), &observed(&[]))
            .with_backend_materialization(Some(
                crate::startup::FRONTEND_READINESS_DEADLINE_ELAPSED.to_string(),
            ));

        assert_eq!(report.unobserved, 1);
        assert!(!report.every_pane_has_a_live_agent());
        let back: RestoreReport =
            serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
        assert_eq!(
            back.backend_materialization.as_deref(),
            Some(crate::startup::FRONTEND_READINESS_DEADLINE_ELAPSED)
        );
        let plain = build_restore_report(&bound, &ids(&["worker"]), &observed(&[]));
        let absent: RestoreReport =
            serde_json::from_str(&serde_json::to_string(&plain).unwrap()).unwrap();
        assert_eq!(absent.backend_materialization, None);
    }
}
