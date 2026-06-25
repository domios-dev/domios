//! Agent hierarchy: roles, reports, status queries.

use serde::Serialize;
use tauri::Emitter;

use super::{ReportResult, ServiceContext};
use crate::agent::ledger::LedgerRow;
use crate::agent::reports::{ReportDetails, ReportStatus};
use crate::multiplexer::types::AgentRole;
use crate::plan::AgentRef;

impl ServiceContext {
    pub fn set_pane_role(
        &self,
        pane_id: &str,
        role_str: &str,
        parent_pane_id: Option<&str>,
        can_spawn: Option<bool>,
    ) -> Result<(), String> {
        let role = Self::parse_role(role_str)?;
        let mut mux = self.mux.write();
        mux.set_pane_role(pane_id, role, parent_pane_id)?;
        if let Some(cs) = can_spawn {
            if let Err(e) = mux.set_can_spawn(pane_id, cs) {
                log::warn!("[hierarchy] set_can_spawn({}) failed: {}", pane_id, e);
            }
        }
        drop(mux);
        self.emit_state_changed();
        Ok(())
    }

    pub fn clear_pane_role(&self, pane_id: &str) -> Result<(), String> {
        self.mux.write().clear_pane_role(pane_id)?;
        self.emit_state_changed();
        Ok(())
    }

    pub fn set_can_spawn(&self, pane_id: &str, can_spawn: bool) -> Result<(), String> {
        self.mux.write().set_can_spawn(pane_id, can_spawn)?;
        self.emit_state_changed();
        Ok(())
    }

    /// Post-condition for spawn: `--parent` on a Worker must land on the pane.
    pub(crate) fn expect_spawn_parent_link(
        &self,
        pane_id: &str,
        expected_parent: &str,
    ) -> Result<(), String> {
        let mux = self.mux.read();
        let pane = mux
            .get_pane(pane_id)
            .ok_or_else(|| format!("Pane {} not found after spawn role apply", pane_id))?;
        match pane.parent_pane_id.as_deref() {
            Some(p) if p == expected_parent => Ok(()),
            Some(p) => Err(format!(
                "spawn parent link mismatch on {pane_id}: expected {expected_parent}, got {p}"
            )),
            None => Err(format!(
                "spawn dropped parent_pane_id on {pane_id}: expected parent {expected_parent}"
            )),
        }
    }

    /// Send report to the resolved target. Routing priority:
    /// `parent_pane_id` → `report_target_pane_id` → `spawned_by_pane_id`
    /// → explicit error. NEVER routes back to the reporter's own pane.
    pub fn submit_report(
        &self,
        pane_id: &str,
        message: &str,
        details: ReportDetails,
        plan_id: Option<&str>,
        task_id: Option<&str>,
        milestone_kind: Option<&str>,
    ) -> Result<ReportResult, String> {
        let (from_title, default_target_opt, model, from_ref, role) = {
            let mux = self.mux.read();
            let (from_title, target_opt) = match resolve_report_target(&mux, pane_id) {
                Ok(resolved) => resolved,
                Err(_) if plan_id.is_some() => (pane_id.to_string(), None),
                Err(e) => return Err(e),
            };
            let pane = mux.get_pane(pane_id);
            let model = pane.and_then(|p| p.agent_session_model.clone());
            let from_ref = pane.map_or_else(
                || AgentRef::from_pane(pane_id),
                |p| AgentRef {
                    pane_id: Some(pane_id.to_string()),
                    session_id: p.agent_session_id.clone(),
                    title: Some(p.title.clone()),
                },
            );
            let role = pane.and_then(|p| p.role);
            (from_title, target_opt, model, from_ref, role)
        };
        let target_opt = plan_id
            .and_then(|id| self.plan_store.owner_of(id))
            .and_then(|owner| owner.pane_id)
            .filter(|target| !target.is_empty() && target != pane_id)
            .or(default_target_opt);

        if let Some(plan_id) = plan_id {
            let task_id = task_id
                .map(str::to_string)
                .or_else(|| {
                    self.plan_store.task_binding_for_pane(pane_id).and_then(
                        |(bound_plan, bound_task)| {
                            if bound_plan == plan_id {
                                Some(bound_task)
                            } else {
                                None
                            }
                        },
                    )
                })
                .ok_or_else(|| "task_id required for plan report".to_string())?;
            let mut ops = Vec::new();
            if matches!(details.status, Some(ReportStatus::InReview)) {
                ops.push(crate::service::plan::batch::BatchOp::SetTaskStatus {
                    client_id: None,
                    task: task_id.clone(),
                    status: "in_review".into(),
                });
            }
            ops.push(crate::service::plan::batch::BatchOp::AddMilestone {
                client_id: Some("report".into()),
                task: task_id,
                kind: milestone_kind.unwrap_or("status_update").into(),
                text: milestone_text(message, details.status),
                from: Some(from_ref),
                key: None,
                links: vec![],
            });
            self.plan_store
                .apply_batch(
                    plan_id,
                    crate::service::plan::batch::BatchRequest { key: None, ops },
                    epoch_ms(),
                )
                .map_err(|e| e.to_string())?;
            self.plan_turn_counter.reset(pane_id);
            crate::api::events::emit_everywhere(
                &self.app_handle,
                "plan-changed",
                serde_json::json!({ "plan_id": plan_id }),
            );
        }

        let target_id = target_opt.ok_or_else(|| {
            "No report target for this pane. You are not attached to an orchestrator, report target, or spawning pane. Ask for the target pane id, then use `codeterm send --pane <id> \"...\"`.".to_string()
        })?;
        let storage_key = target_id.clone();
        let report = self.report_store.lock().add_detailed(
            &storage_key,
            pane_id,
            &from_title,
            message,
            details,
            model.as_deref(),
        );

        self.events.broadcast(
            "agent.report",
            serde_json::json!({
                "target_pane_id": storage_key,
                "report": report,
            }),
        );
        let _ = self.app_handle.emit(
            "agent-report",
            serde_json::json!({
                "target_pane_id": storage_key,
                "from_pane_id": pane_id,
                "from_title": from_title,
                "message": message,
            }),
        );

        let envelope = crate::service::messenger::MessageEnvelope::report(&from_title, pane_id)
            .model(model.clone());
        let delivery = crate::service::messenger::Messenger::deliver(
            self,
            &target_id,
            envelope,
            &report.inject_body(),
        )?;
        log::info!("[report] target={} delivered via {:?}", target_id, delivery);

        if plan_id.is_none() {
            self.maybe_send_plan_reminder(pane_id, role);
        }

        Ok(ReportResult {
            report_id: report.id,
            orchestrator_pane_id: storage_key,
        })
    }

    /// §EFFORT: moving a task to `in_review` IS the trigger — the plan system itself
    /// notifies the plan owner (orchestrator) so a worker need not send a separate
    /// `agent report`. The status change carries the intent; this delivers it. Best-effort.
    pub fn notify_owner_task_in_review(
        &self,
        plan_id: &str,
        task_id: &str,
        by_pane: Option<&str>,
        intent: Option<&str>,
    ) {
        let Some(plan) = self.plan_store.get(plan_id) else {
            return;
        };
        let Some(owner) = plan.owner.pane_id.as_deref() else {
            return;
        };
        if Some(owner) == by_pane {
            return; // no self-notify
        }
        let task_name = plan
            .tasks
            .iter()
            .find(|t| t.id == task_id)
            .map(|t| t.name.clone())
            .unwrap_or_else(|| task_id.to_string());
        let who = by_pane.unwrap_or("a worker");
        let intent_part = intent
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| format!(" — {s}"))
            .unwrap_or_default();
        let text = format!(
            "[task in_review] '{task_name}' ({task_id}) → in_review by {who}{intent_part}\nReview it, then judge done or bounce back."
        );
        let envelope = crate::service::messenger::MessageEnvelope::reminder("CodeTerm", "plan");
        let _ = crate::service::messenger::Messenger::deliver(self, owner, envelope, &text);
    }

    fn maybe_send_plan_reminder(&self, pane_id: &str, role: Option<AgentRole>) {
        if self.plan_store.task_binding_for_pane(pane_id).is_none() {
            return;
        }
        let (turns, last_nudge) = self.plan_turn_counter.increment(pane_id);
        let cfg = crate::config::load_config().orchestration;
        let nudge_cfg = crate::service::plan::reminders::NudgeConfig {
            plan_reminders: cfg.plan_reminders,
            nudge_after_turns: cfg.nudge_after_turns,
            role,
        };
        let Some(kind) =
            crate::service::plan::reminders::should_nudge(turns, last_nudge, &nudge_cfg)
        else {
            return;
        };
        let text = crate::service::plan::reminders::nudge_text(kind);
        let envelope = crate::service::messenger::MessageEnvelope::reminder("CodeTerm", "system");
        if crate::service::messenger::Messenger::deliver(self, pane_id, envelope, text).is_ok() {
            self.plan_turn_counter.mark_nudged(pane_id, turns);
        }
    }
    pub fn get_reports(&self, pane_id: &str) -> Vec<serde_json::Value> {
        let reports = self.report_store.lock().get(pane_id, false);
        serde_json::to_value(&reports)
            .unwrap_or_default()
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_and_clear_reports(&self, pane_id: &str) -> Vec<serde_json::Value> {
        let reports = self.report_store.lock().get(pane_id, true);
        serde_json::to_value(&reports)
            .unwrap_or_default()
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    /// Durable per-orchestrator ledger rows (survive restart) from the sqlite ledger,
    /// unlike [`get_reports`](Self::get_reports) which reads the in-memory stream.
    pub fn get_ledger(&self, orchestrator_pane_id: &str) -> Vec<LedgerRow> {
        let rows = self.report_store.lock().ledger_rows(orchestrator_pane_id);
        let mux = self.mux.read();
        ledger_with_live_children(&mux, orchestrator_pane_id, rows, epoch_seconds())
    }

    pub fn get_pane_status(&self, pane_id: &str) -> Result<PaneStatusInfo, String> {
        let mux = self.mux.write();
        let pane = mux
            .get_pane(pane_id)
            .ok_or(format!("Pane not found: {}", pane_id))?;

        let status = if let Some(ref pty_id) = pane.pty_id {
            let tracker = self.agent_tracker.lock();
            format!("{:?}", tracker.get_status(pty_id))
        } else {
            "Idle".to_string()
        };

        let children = mux.get_children_ids(pane_id);

        Ok(PaneStatusInfo {
            pane_id: pane.id.clone(),
            status,
            agent_type: pane.agent_type.clone(),
            role: pane.role,
            parent_pane_id: pane.parent_pane_id.clone(),
            can_spawn: pane.can_spawn,
            depth: pane.depth,
            children,
            cwd: pane.cwd.clone(),
            title: pane.title.clone(),
        })
    }
}

fn milestone_text(message: &str, status: Option<ReportStatus>) -> String {
    match status {
        Some(ReportStatus::Done) => format!("DONE: {message}"),
        Some(ReportStatus::InReview) => format!("IN_REVIEW: {message}"),
        Some(ReportStatus::Partial) => format!("PARTIAL: {message}"),
        Some(ReportStatus::Blocked) => format!("BLOCKED: {message}"),
        None => message.to_string(),
    }
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn epoch_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn ledger_with_live_children(
    mux: &crate::multiplexer::state::MultiplexerState,
    orchestrator_pane_id: &str,
    mut rows: Vec<LedgerRow>,
    now: u64,
) -> Vec<LedgerRow> {
    for child_id in mux.get_children_ids(orchestrator_pane_id) {
        let Some(child) = mux.get_pane(&child_id) else {
            continue;
        };
        let title = mux.display_label_for_pane(&child_id);
        match rows.iter_mut().find(|r| r.worker_pane_id == child_id) {
            Some(row) => {
                row.title = title;
                row.model = child
                    .agent_session_model
                    .clone()
                    .or_else(|| row.model.clone());
                row.updated_at = row.updated_at.max(now);
            }
            None => rows.push(LedgerRow {
                orchestrator_pane_id: orchestrator_pane_id.to_string(),
                worker_pane_id: child_id,
                title,
                branch: None,
                model: child.agent_session_model.clone(),
                status: None,
                message: String::new(),
                sha: None,
                changed_files: None,
                test_cmd: None,
                test_result: None,
                skipped: None,
                updated_at: now,
            }),
        }
    }
    rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    rows
}

/// Resolve where a report from `pane_id` should be delivered.
///
/// Returns `(from_title, Some(target_pane_id))` when a real target exists,
/// or `(from_title, None)` when the report has no implicit recipient and must
/// fail with an explicit `codeterm send --pane <id>` hint. The resolution order is:
///
/// 1. `parent_pane_id` — direct parent in the role hierarchy (cascades up)
/// 2. `report_target_pane_id` — cron / mem worker non-hierarchy target
/// 3. `spawned_by_pane_id` — agent that ran `agent spawn`, even when no
///    role parent (top-level Orchestrator spawned by another agent)
///
/// The reporter's own pane id is filtered out at every level: a pane must
/// never resolve to itself, otherwise reports get injected into the
/// reporter's own buffer and never reach the spawning agent.
pub fn resolve_report_target(
    mux: &crate::multiplexer::state::MultiplexerState,
    pane_id: &str,
) -> Result<(String, Option<String>), String> {
    let pane = mux.get_pane(pane_id).ok_or("Pane not found")?;
    let title = mux.display_label_for_pane(pane_id);
    let target = [
        pane.parent_pane_id.as_deref(),
        pane.report_target_pane_id.as_deref(),
        pane.spawned_by_pane_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .find(|candidate| !candidate.is_empty() && *candidate != pane_id)
    .map(str::to_string);
    Ok((title, target))
}

/// Whether a dying pane needs a synthetic worker-death report, and to whom — a
/// worker whose PTY dies without ever reporting leaves its orchestrator blind.
/// Returns `(target, message)` for a hierarchy worker that hasn't already reported
/// to that target; `None` for non-hierarchy panes or workers that already reported.
pub fn worker_death_report(
    mux: &crate::multiplexer::state::MultiplexerState,
    report_store: &crate::agent::reports::ReportStore,
    pane_id: &str,
) -> Option<(String, String)> {
    let (_from_title, target) = resolve_report_target(mux, pane_id).ok()?;
    let target_id = target?;
    if report_store.has_report_from(&target_id, pane_id) {
        return None;
    }
    Some((
        target_id,
        format!("WORKER {} EXITED without reporting (dead)", pane_id),
    ))
}

#[derive(Debug, Clone, Serialize)]
pub struct PaneStatusInfo {
    pub pane_id: String,
    pub status: String,
    pub agent_type: Option<String>,
    pub role: Option<AgentRole>,
    pub parent_pane_id: Option<String>,
    pub can_spawn: bool,
    pub depth: u8,
    pub children: Vec<String>,
    pub cwd: Option<String>,
    pub title: String,
}

#[cfg(test)]
mod tests {
    use super::{
        ledger_with_live_children, milestone_text, resolve_report_target, worker_death_report,
    };
    use crate::agent::ledger::LedgerRow;
    use crate::agent::reports::{ReportStatus, ReportStore};
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::AgentRole;

    // Enforce report key symmetry: reporter writes and parent query must use the same key.
    // This avoids "report succeeded" with empty parent results.

    fn build_orchestrator_with_worker(worker_title: &str) -> (MultiplexerState, String, String) {
        let (mut mux, orch) = MultiplexerState::new();
        let worker = mux.create_pane();
        mux.set_pane_role(&worker, AgentRole::Worker, Some(&orch))
            .expect("attach worker to orchestrator");
        mux.rename_pane(&worker, worker_title)
            .expect("rename worker pane");
        (mux, orch, worker)
    }

    #[test]
    fn worker_report_is_listed_by_orchestrator() {
        let (mux, orch, worker) = build_orchestrator_with_worker("Worker-1");
        let mut store = ReportStore::new(100);

        let (from_title, target_id) = resolve_report_target(&mux, &worker).expect("resolve target");
        let target_id = target_id.expect("worker has a parent target");
        assert_eq!(target_id, orch, "worker reports to its direct parent");
        assert_eq!(from_title, "Worker-1");

        store.add(&target_id, &worker, &from_title, "task done");

        let listed = store.get(&orch, false);
        assert_eq!(
            listed.len(),
            1,
            "orchestrator must see the worker's report after submit"
        );
        assert_eq!(listed[0].from_pane_id, worker);
        assert_eq!(listed[0].from_title, "Worker-1");
        assert_eq!(listed[0].message, "task done");
    }

    #[test]
    fn worker_report_keys_match_storage_and_query() {
        let (mux, orch, worker) = build_orchestrator_with_worker("Worker-A");
        let mut store = ReportStore::new(100);

        let (from_title, storage_key) =
            resolve_report_target(&mux, &worker).expect("resolve target");
        let storage_key = storage_key.expect("worker has a parent target");
        store.add(&storage_key, &worker, &from_title, "msg");

        let query_key = &orch;
        assert_eq!(
            &storage_key, query_key,
            "submit_report write key and agent.reports query key must agree"
        );
        assert_eq!(store.get(query_key, false).len(), 1);
    }

    #[test]
    fn ledger_includes_live_child_before_first_report() {
        let (mut mux, orch, worker) = build_orchestrator_with_worker("ledger-smoke");
        mux.get_pane_mut(&worker).unwrap().agent_session_model = Some("gpt-5.5".into());

        let rows = ledger_with_live_children(&mux, &orch, vec![], 123);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].orchestrator_pane_id, orch);
        assert_eq!(rows[0].worker_pane_id, worker);
        assert_eq!(rows[0].title, "ledger-smoke");
        assert_eq!(rows[0].model.as_deref(), Some("gpt-5.5"));
        assert_eq!(rows[0].status, None);
        assert_eq!(rows[0].message, "");
        assert_eq!(rows[0].updated_at, 123);
    }

    #[test]
    fn ledger_refreshes_existing_live_child_without_losing_report_details() {
        let (mut mux, orch, worker) = build_orchestrator_with_worker("fresh-title");
        mux.get_pane_mut(&worker).unwrap().agent_session_model = Some("sonnet".into());
        let rows = vec![LedgerRow {
            orchestrator_pane_id: orch.clone(),
            worker_pane_id: worker.clone(),
            title: "stale-title".into(),
            branch: Some("lane/agent-ledger".into()),
            model: Some("old-model".into()),
            status: Some(ReportStatus::Partial),
            message: "still working".into(),
            sha: Some("abc123".into()),
            changed_files: Some(vec!["src-tauri/src/service/hierarchy.rs".into()]),
            test_cmd: Some("cargo test hierarchy".into()),
            test_result: Some("ok".into()),
            skipped: None,
            updated_at: 10,
        }];

        let rows = ledger_with_live_children(&mux, &orch, rows, 99);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "fresh-title");
        assert_eq!(rows[0].model.as_deref(), Some("sonnet"));
        assert_eq!(rows[0].branch.as_deref(), Some("lane/agent-ledger"));
        assert_eq!(rows[0].status, Some(ReportStatus::Partial));
        assert_eq!(rows[0].message, "still working");
        assert_eq!(rows[0].updated_at, 99);
    }

    #[test]
    fn no_plan_routing_keeps_parent_priority() {
        let (mux, orch, worker) = build_orchestrator_with_worker("Worker-A");
        let (_from_title, storage_key) =
            resolve_report_target(&mux, &worker).expect("resolve target");
        assert_eq!(storage_key.as_deref(), Some(orch.as_str()));
    }

    #[test]
    fn report_title_uses_user_pinned_pane_identity_not_provider_default() {
        let (mut mux, orch) = MultiplexerState::new();
        let ws = mux
            .find_workspace_for_pane(&orch)
            .expect("orchestrator lives in a workspace");
        let (_tab_id, worker) = mux.create_tab(&ws).expect("create worker tab");
        mux.set_pane_role(&worker, AgentRole::Worker, Some(&orch))
            .expect("attach worker");
        mux.rename_pane(&worker, "Euler")
            .expect("set pane identity");
        mux.rename_pane_at_level(
            &worker,
            "claude",
            crate::multiplexer::types::TitleLevel::Agent,
        )
        .expect("agent title ignored below user pin");

        let (from_title, target) = resolve_report_target(&mux, &worker).expect("resolve");
        assert_eq!(target.as_deref(), Some(orch.as_str()));
        assert_eq!(
            from_title, "Euler",
            "report must carry the tab identity, not the provider default"
        );
    }

    #[test]
    fn milestone_text_prefixes_structured_status_only() {
        assert_eq!(milestone_text("shipped", None), "shipped");
        assert_eq!(
            milestone_text("shipped", Some(ReportStatus::Done)),
            "DONE: shipped"
        );
        assert_eq!(
            milestone_text("needs help", Some(ReportStatus::Blocked)),
            "BLOCKED: needs help"
        );
        assert_eq!(
            milestone_text("ready", Some(ReportStatus::InReview)),
            "IN_REVIEW: ready"
        );
    }

    #[test]
    fn cron_report_target_is_used_when_no_parent() {
        let (mut mux, orch) = MultiplexerState::new();
        let cron_pane = mux.create_pane();
        // Cron-spawned panes carry an explicit report_target_pane_id but no role.
        if let Some(pane) = mux.get_pane_mut(&cron_pane) {
            pane.report_target_pane_id = Some(orch.clone());
        }
        mux.rename_pane(&cron_pane, "cron-task").unwrap();

        let (from_title, target) = resolve_report_target(&mux, &cron_pane).unwrap();
        assert_eq!(target.as_deref(), Some(orch.as_str()));
        assert_eq!(from_title, "cron-task");
    }

    #[test]
    fn parent_pane_id_takes_priority_over_other_targets() {
        let (mut mux, orch, worker) = build_orchestrator_with_worker("Worker-Pri");
        let other = mux.create_pane();
        let spawner = mux.create_pane();
        if let Some(pane) = mux.get_pane_mut(&worker) {
            pane.report_target_pane_id = Some(other.clone());
            pane.spawned_by_pane_id = Some(spawner.clone());
        }

        let (_, target) = resolve_report_target(&mux, &worker).unwrap();
        assert_eq!(
            target.as_deref(),
            Some(orch.as_str()),
            "explicit parent_pane_id wins over report_target and spawned_by"
        );
    }

    #[test]
    fn orchestrator_falls_back_to_spawned_by_when_no_parent() {
        let (mut mux, orch) = MultiplexerState::new();
        // Simulate the spawning agent — it lives in its own pane elsewhere.
        let spawner = mux.create_pane();
        // Top-level orchestrator has no parent and no report_target, but `agent spawn` recorded who asked for it.
        if let Some(pane) = mux.get_pane_mut(&orch) {
            pane.spawned_by_pane_id = Some(spawner.clone());
        }

        let (_, target) = resolve_report_target(&mux, &orch).unwrap();
        assert_eq!(
            target.as_deref(),
            Some(spawner.as_str()),
            "top-level orchestrator routes its report to whoever spawned it"
        );
    }

    #[test]
    fn never_routes_back_to_self_when_all_targets_unset() {
        // Top-level orchestrator with no parent / no report_target / no spawner. The previous implementation walked the parent chain and returned the
        // pane's own id, so reports got injected into the orchestrator's own PTY and `agent reports` against the spawning agent stayed empty.
        let (mux, orch) = MultiplexerState::new();
        let (_, target) = resolve_report_target(&mux, &orch).unwrap();
        assert!(
            target.is_none(),
            "orphan orchestrator must NOT resolve to itself; got {:?}",
            target
        );
    }

    #[test]
    fn never_routes_back_to_self_even_when_targets_point_at_self() {
        // Defensive: if any field accidentally points at the reporter itself, that field is skipped — reports should never bounce.
        let (mut mux, orch) = MultiplexerState::new();
        if let Some(pane) = mux.get_pane_mut(&orch) {
            pane.parent_pane_id = Some(orch.clone());
            pane.report_target_pane_id = Some(orch.clone());
            pane.spawned_by_pane_id = Some(orch.clone());
        }
        let (_, target) = resolve_report_target(&mux, &orch).unwrap();
        assert!(
            target.is_none(),
            "self-referential targets must be filtered out; got {:?}",
            target
        );
    }

    #[test]
    fn spawned_by_used_only_when_parent_and_report_target_missing() {
        let (mut mux, orch) = MultiplexerState::new();
        let spawner = mux.create_pane();
        let report_target = mux.create_pane();
        if let Some(pane) = mux.get_pane_mut(&orch) {
            pane.report_target_pane_id = Some(report_target.clone());
            pane.spawned_by_pane_id = Some(spawner.clone());
        }
        let (_, target) = resolve_report_target(&mux, &orch).unwrap();
        assert_eq!(
            target.as_deref(),
            Some(report_target.as_str()),
            "report_target_pane_id wins over spawned_by_pane_id"
        );
    }

    #[test]
    fn unknown_pane_id_is_rejected() {
        let (mux, _orch) = MultiplexerState::new();
        let err = resolve_report_target(&mux, "nonexistent").unwrap_err();
        assert!(err.contains("Pane not found"), "got: {}", err);
    }

    // ── Worker-death watchdog ──

    #[test]
    fn dead_worker_without_report_routes_synthetic_to_parent() {
        let (mux, orch, worker) = build_orchestrator_with_worker("Worker-Dead");
        let store = ReportStore::new(100);

        let (target, msg) = worker_death_report(&mux, &store, &worker)
            .expect("dead unreported worker must synthesize a report");
        assert_eq!(target, orch, "synthetic dead-report routes to the parent");
        assert_eq!(
            msg,
            format!("WORKER {} EXITED without reporting (dead)", worker)
        );
    }

    #[test]
    fn dead_worker_that_already_reported_gets_no_synthetic() {
        let (mux, orch, worker) = build_orchestrator_with_worker("Worker-Reported");
        let mut store = ReportStore::new(100);
        // Worker already spoke to its parent before dying.
        store.add(&orch, &worker, "Worker-Reported", "task done");

        assert!(
            worker_death_report(&mux, &store, &worker).is_none(),
            "a worker that already reported must NOT get a synthetic dead-report"
        );
    }

    #[test]
    fn dead_non_hierarchy_pane_gets_no_synthetic() {
        // Plain pane: no parent/report_target/spawner → no routing target → no synthetic report.
        let (mut mux, _orch) = MultiplexerState::new();
        let plain = mux.create_pane();
        let store = ReportStore::new(100);

        assert!(
            worker_death_report(&mux, &store, &plain).is_none(),
            "a non-hierarchy pane must NOT get a synthetic dead-report"
        );
    }

    #[test]
    fn dead_worker_routes_via_spawned_by_when_no_parent() {
        // A top-level pane spawned by another agent still routes its death to whoever spawned it.
        let (mut mux, _orch) = MultiplexerState::new();
        let spawner = mux.create_pane();
        let worker = mux.create_pane();
        if let Some(pane) = mux.get_pane_mut(&worker) {
            pane.spawned_by_pane_id = Some(spawner.clone());
        }
        let store = ReportStore::new(100);

        let (target, _msg) =
            worker_death_report(&mux, &store, &worker).expect("spawned_by gives a target");
        assert_eq!(target, spawner);
    }

    // ── Report message format ── submit_report builds the notify body without a trailing CR — Enter is sent as a separate delayed raw write by send_text's bracket-paste path.

    fn format_report(from_title: &str, message: &str) -> String {
        format!("[Report from {}] {}", from_title, message)
    }

    #[test]
    fn report_format_basic() {
        let msg = format_report("Scout", "Task completed successfully");
        assert_eq!(msg, "[Report from Scout] Task completed successfully");
    }

    #[test]
    fn report_format_no_trailing_cr() {
        // Enter is delivered as a separate write by send_text — the body itself must NOT contain trailing \r, otherwise Claude's prompt submits before
        // the bracket-paste end marker is processed.
        let msg = format_report("Worker-1", "done");
        assert!(!msg.ends_with('\r'), "Report body must not end with \\r");
    }

    #[test]
    fn report_format_message_with_newlines() {
        // Internal newlines pass through raw — bracket paste protects them.
        let msg = format_report("Worker", "line1\nline2\nline3");
        assert_eq!(msg, "[Report from Worker] line1\nline2\nline3");
        assert!(msg.contains('\n'), "Internal newlines are preserved");
    }

    #[test]
    fn report_format_empty_message() {
        let msg = format_report("Agent", "");
        assert_eq!(msg, "[Report from Agent] ");
    }

    #[test]
    fn report_format_title_with_spaces() {
        let msg = format_report("My Agent Title", "ok");
        assert_eq!(msg, "[Report from My Agent Title] ok");
    }

    #[test]
    fn report_format_message_with_special_chars() {
        let msg = format_report("Bot", "found 3 files: a.rs, b.rs, c.rs");
        assert!(msg.starts_with("[Report from Bot] "));
    }
}
