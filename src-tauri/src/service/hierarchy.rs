//! Agent hierarchy: roles, reports, status queries.

mod plan_reminder_nudge;

use serde::Serialize;

use super::{ReportResult, ServiceContext, ServiceState};
use crate::agent::ledger::LedgerRow;
use crate::agent::reports::{ReportDetails, ReportStatus};
use crate::multiplexer::types::AgentRole;
use crate::plan::AgentRef;
use tauri::Manager;

pub(crate) fn start_plan_reminder_scheduler(svc: ServiceState) {
    plan_reminder_nudge::start_plan_reminder_scheduler(svc);
}

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
        match self.mux.write().clear_pane_role(pane_id) {
            Ok(()) => {
                self.emit_state_changed();
                Ok(())
            }
            Err(e) if e.contains("not found") => {
                // Cross-mesh workers live only in RemoteWorkerRegistry — their
                // foreign pane id is what `agent unlink --pane` passes.
                let touched = self
                    .remote_workers
                    .forget_by_remote_pane(pane_id, None, None);
                if touched.is_empty() {
                    Err(e)
                } else {
                    for orch in touched {
                        self.emit_remote_workers_changed(&orch);
                    }
                    Ok(())
                }
            }
            Err(e) => Err(e),
        }
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
    /// remote mesh target (`report_target_mesh_peer` + pane/alias) →
    /// `parent_pane_id` → `report_target_pane_id` → `spawned_by_pane_id`.
    /// NEVER routes back to the reporter's own pane; errors when none resolve.
    ///
    /// Plan/task resolution order:
    /// 1. Explicit `plan_id`/`task_id` from the caller — validated against live store.
    /// 2. If provided ids are stale (plan not found) — falls back to pane assignment.
    /// 3. Daemon-side pane assignment lookup (`task_bindings_with_name_for_pane`).
    /// 4. Ambiguous (>1 assignment) — errors with candidate list.
    /// 5. No assignment — proceeds without a plan milestone.
    pub fn submit_report(
        &self,
        pane_id: &str,
        message: &str,
        details: ReportDetails,
        plan_id: Option<&str>,
        task_id: Option<&str>,
        milestone_kind: Option<&str>,
    ) -> Result<ReportResult, String> {
        let task_status = report_task_status_transition(details.status)?;

        let mut notices: Vec<String> = Vec::new();
        let (eff_plan_id, eff_task_id) =
            resolve_plan_ctx(&self.plan_store, pane_id, plan_id, task_id, &mut notices)?;

        let (from_title, default_target_opt, model, from_ref, role) = {
            let mux = self.mux.read();
            let (from_title, target_opt) = match resolve_report_target(&mux, pane_id) {
                Ok(resolved) => resolved,
                Err(_) if eff_plan_id.is_some() => (pane_id.to_string(), None),
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
        // Capture mesh peer before default_target_opt is consumed by .or().
        let default_mesh_peer = match &default_target_opt {
            Some(ReportTarget::Mesh { peer, .. }) => Some(peer.clone()),
            _ => None,
        };

        let target_opt = eff_plan_id
            .as_deref()
            .and_then(|id| self.plan_store.owner_of(id))
            .and_then(|owner| owner.pane_id)
            .filter(|target| !target.is_empty() && target != pane_id)
            .map(ReportTarget::Local)
            .or(default_target_opt);

        // Mesh homing: plan_id hint provided but plan not found locally (remote worker scenario).
        // Relay plan batch ops (status + milestone) to the owning peer via the mesh.
        if eff_plan_id.is_none() {
            if let (Some(h_plan), Some(h_task), Some(peer)) = (
                plan_id.filter(|s| !s.trim().is_empty()),
                task_id.filter(|s| !s.trim().is_empty()),
                default_mesh_peer,
            ) {
                self.relay_plan_batch_to_peer(
                    peer,
                    h_plan.to_string(),
                    h_task.to_string(),
                    from_ref.clone(),
                    task_status,
                    milestone_text(message, details.status),
                    milestone_kind.unwrap_or("status_update").to_string(),
                );
            }
        }

        if let Some(ref plan_id) = eff_plan_id {
            let task_id = eff_task_id
                .clone()
                .or_else(|| {
                    self.plan_store.task_binding_for_pane(pane_id).and_then(
                        |(bound_plan, bound_task)| {
                            if &bound_plan == plan_id {
                                Some(bound_task)
                            } else {
                                None
                            }
                        },
                    )
                })
                .ok_or_else(|| "task_id required for plan report".to_string())?;
            let mut ops = Vec::new();
            if let Some(status) = task_status {
                ops.push(crate::service::plan::batch::BatchOp::SetTaskStatus {
                    client_id: None,
                    task: task_id.clone(),
                    status: status.into(),
                });
            }
            ops.push(crate::service::plan::batch::BatchOp::AddMilestone {
                client_id: Some("report".into()),
                task: task_id.clone(),
                kind: milestone_kind.unwrap_or("status_update").into(),
                text: milestone_text(message, details.status),
                from: Some(from_ref.clone()),
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
            let _ = self.plan_store.record_task_attempt_report(
                plan_id,
                &task_id,
                &from_ref,
                message,
                details.status,
                epoch_ms(),
            );
            self.plan_turn_counter.reset(pane_id);
            crate::api::events::emit_everywhere(
                &self.app_handle,
                "plan-changed",
                serde_json::json!({ "plan_id": plan_id }),
            );
        }

        let target = target_opt.ok_or_else(|| {
            "No report target for this pane. You are not attached to an orchestrator, report target, or spawning pane. Ask for the target pane id, then use `codeterm send [--mesh <peer>] --pane <pane-or-alias> \"...\"`, or spawn with a report target.".to_string()
        })?;
        let storage_key = target.storage_key();
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
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "agent-report",
            serde_json::json!({
                "target_pane_id": storage_key,
                "from_pane_id": pane_id,
                "from_title": from_title,
                "message": message,
            }),
        );

        match target {
            // Remote worker: the real orchestrator/General target lives on a
            // paired peer. Reuse the signed mesh `/send` relay so the report
            // surfaces there exactly like a local `[Report from <worker>@<peer>]`.
            ReportTarget::Mesh { peer, pane } => {
                self.relay_report_to_peer(
                    peer,
                    pane,
                    from_title,
                    pane_id.to_string(),
                    model,
                    report.inject_body(),
                );
            }
            ReportTarget::Local(target_id) => {
                let envelope =
                    crate::service::messenger::MessageEnvelope::report(&from_title, pane_id)
                        .model(model.clone());
                let delivery = crate::service::messenger::Messenger::deliver(
                    self,
                    &target_id,
                    envelope,
                    &report.inject_body(),
                )?;
                log::info!("[report] target={} delivered via {:?}", target_id, delivery);
            }
        }

        if eff_plan_id.is_none() {
            self.maybe_send_plan_reminder(pane_id, role);
        }

        Ok(ReportResult {
            report_id: report.id,
            orchestrator_pane_id: storage_key,
            notices,
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
        let text = self.plan_store.prepend_heartbeat_for_plan(plan_id, &text);
        let envelope = crate::service::messenger::MessageEnvelope::reminder("CodeTerm", "plan");
        let _ = crate::service::messenger::Messenger::deliver(self, owner, envelope, &text);
    }

    /// Fire-and-forget relay of plan batch ops (status update + milestone) to the
    /// peer that owns the plan. Used when the plan doesn't exist in the local store
    /// but plan_id + task_id were provided (remote worker mesh homing scenario).
    fn relay_plan_batch_to_peer(
        &self,
        peer: String,
        plan_id: String,
        task_id: String,
        from_ref: AgentRef,
        task_status: Option<&'static str>,
        ms_text: String,
        ms_kind: String,
    ) {
        let client = {
            let mesh_handle = self.app_handle.state::<crate::MeshStateHandle>();
            let mesh = mesh_handle.lock();
            resolve_peer_client_by_name(&mesh, &peer)
        };
        let Some(client) = client else {
            log::warn!(
                "[plan/mesh] no paired peer '{}' — plan batch for {} not relayed",
                peer,
                plan_id
            );
            return;
        };
        let mut ops: Vec<serde_json::Value> = Vec::new();
        if let Some(status) = task_status {
            ops.push(serde_json::json!({
                "op": "set_task_status",
                "task": task_id,
                "status": status,
            }));
        }
        ops.push(serde_json::json!({
            "op": "add_milestone",
            "client_id": "report",
            "task": task_id,
            "kind": ms_kind,
            "text": ms_text,
            "from": serde_json::to_value(&from_ref).unwrap_or_default(),
        }));
        let body = serde_json::json!({ "ops": ops });
        let cmd = crate::mesh::types::RelayCommand {
            method: "POST".into(),
            path: format!("/api/plans/{}/batch", plan_id),
            body,
        };
        tauri::async_runtime::spawn(async move {
            match client.send_relay(&cmd).await {
                Ok(r) if r.ok => {
                    log::info!("[plan/mesh] batch relayed to {}/{}", peer, plan_id)
                }
                Ok(r) => log::warn!(
                    "[plan/mesh] {} rejected plan batch for {}: {:?}",
                    peer,
                    plan_id,
                    r.error
                ),
                Err(e) => log::warn!("[plan/mesh] plan batch relay to {} failed: {}", peer, e),
            }
        });
    }

    /// Fire-and-forget delivery of a report to a pane (id or `main`/`general`
    /// alias) on a paired mesh peer. Resolves the peer by instance name or id,
    /// then rides the same signed relay the CLI `--mesh` send uses; the peer's
    /// relay ingress stamps the authenticated origin so the report renders as
    /// `[Report from <worker>@<peer>]`. Errors are logged, never surfaced — the
    /// report is already stored locally and the reporter must not block.
    fn relay_report_to_peer(
        &self,
        peer: String,
        target_pane: String,
        from_title: String,
        from_pane: String,
        model: Option<String>,
        body: String,
    ) {
        let client = {
            let mesh_handle = self.app_handle.state::<crate::MeshStateHandle>();
            let mesh = mesh_handle.lock();
            resolve_peer_client_by_name(&mesh, &peer)
        };
        let Some(client) = client else {
            log::warn!(
                "[report/mesh] no paired peer named '{}' — report from {} stored only",
                peer,
                from_pane
            );
            return;
        };
        let mut send_body = serde_json::json!({
            "text": body,
            "source_pane": from_pane,
            "source_label": from_title,
            "kind": "report",
            "enter": true,
        });
        if let Some(m) = model.filter(|m| !m.trim().is_empty()) {
            send_body["source_model"] = serde_json::json!(m);
        }
        let cmd = crate::mesh::types::RelayCommand {
            method: "POST".into(),
            path: format!("/api/panes/{}/send", target_pane),
            body: send_body,
        };
        tauri::async_runtime::spawn(async move {
            match client.send_relay(&cmd).await {
                Ok(r) if r.ok => log::info!("[report/mesh] delivered to {}/{}", peer, target_pane),
                Ok(r) => log::warn!(
                    "[report/mesh] {} rejected report to {}: {:?}",
                    peer,
                    target_pane,
                    r.error
                ),
                Err(e) => log::warn!("[report/mesh] relay to {} failed: {}", peer, e),
            }
        });
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

fn report_task_status_transition(
    status: Option<ReportStatus>,
) -> Result<Option<&'static str>, String> {
    match status {
        Some(ReportStatus::Done) => Err(
            "workers must not report --status done because only the owner can close a task. Use --status in_review when the work is ready for review, --status partial for progress, or ask the owner to close after review.".to_string(),
        ),
        Some(ReportStatus::InReview) => Ok(Some("in_review")),
        Some(ReportStatus::Blocked) => Ok(Some("blocked")),
        Some(ReportStatus::Partial) | None => Ok(None),
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
                model: child.agent_session_model.clone(),
                updated_at: now,
                ..Default::default()
            }),
        }
    }
    rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    rows
}

/// Where a report should be delivered. A target is either a pane on this same
/// daemon, or a pane (id or `main`/`general` alias) on a paired mesh peer — the
/// latter lets a worker spawned via `agent spawn --mesh PEER` report back to the
/// real orchestrator / General target on another machine without the reporter
/// ever naming the peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportTarget {
    /// A pane on this daemon.
    Local(String),
    /// A pane id or alias on a paired peer, addressed by its instance name/id.
    Mesh { peer: String, pane: String },
}

impl ReportTarget {
    /// Stable key the report store and worker-death dedup index by. Local targets
    /// key on the pane id (so `agent reports --pane <id>` finds them); mesh targets
    /// key on a synthetic `mesh:<peer>/<pane>` so they neither collide with local
    /// panes nor each other.
    pub fn storage_key(&self) -> String {
        match self {
            ReportTarget::Local(id) => id.clone(),
            ReportTarget::Mesh { peer, pane } => format!("mesh:{}/{}", peer, pane),
        }
    }
}

/// Resolve where a report from `pane_id` should be delivered.
///
/// Returns `(from_title, Some(target))` when a real target exists, or
/// `(from_title, None)` when the report has no implicit recipient and must fail
/// with an explicit `codeterm send` hint. The resolution order is:
///
/// 1. `report_target_mesh_peer` (+ pane/alias) — explicit remote target set by
///    `agent spawn --mesh PEER --report-to ...`; wins so a remote worker reports
///    across the mesh, not into a stale local field.
/// 2. `parent_pane_id` — direct parent in the role hierarchy (cascades up)
/// 3. `report_target_pane_id` — cron / mem worker non-hierarchy target
/// 4. `spawned_by_pane_id` — agent that ran `agent spawn`, even when no
///    role parent (top-level Orchestrator spawned by another agent)
///
/// The reporter's own pane id is filtered out at every local level: a pane must
/// never resolve to itself, otherwise reports get injected into the reporter's
/// own buffer and never reach the spawning agent.
pub fn resolve_report_target(
    mux: &crate::multiplexer::state::MultiplexerState,
    pane_id: &str,
) -> Result<(String, Option<ReportTarget>), String> {
    let pane = mux.get_pane(pane_id).ok_or("Pane not found")?;
    let title = mux.display_label_for_pane(pane_id);

    let nonempty = |s: &Option<String>| {
        s.as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    if let (Some(peer), Some(target_pane)) = (
        nonempty(&pane.report_target_mesh_peer),
        nonempty(&pane.report_target_pane_id),
    ) {
        return Ok((
            title,
            Some(ReportTarget::Mesh {
                peer,
                pane: target_pane,
            }),
        ));
    }

    let local = [
        pane.parent_pane_id.as_deref(),
        pane.report_target_pane_id.as_deref(),
        pane.spawned_by_pane_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .find(|candidate| !candidate.is_empty() && *candidate != pane_id)
    .map(|id| ReportTarget::Local(id.to_string()));
    Ok((title, local))
}

/// Resolve a paired peer by instance name (case-insensitive) or instance id and
/// build a signed transport client for it. Name first so the human-facing
/// `--mesh Mac-Air` works; falls back to a raw instance id.
fn resolve_peer_client_by_name(
    mesh: &crate::mesh::manager::MeshManager,
    name: &str,
) -> Option<crate::mesh::transport::PeerClient> {
    if mesh.paired_peers.contains_key(name) {
        return mesh.client_for(name);
    }
    let id = mesh
        .paired_peers
        .iter()
        .find(|(_, p)| p.instance_name.eq_ignore_ascii_case(name))
        .map(|(id, _)| id.clone())?;
    mesh.client_for(&id)
}

/// Resolve the effective (plan_id, task_id) for a report, following the priority chain:
/// 1. Provided `plan_id_hint` validated against live store.
/// 2. If stale (plan not found), falls back to daemon-side pane assignment.
/// 3. If no `plan_id_hint`, tries pane assignment directly.
/// 4. Ambiguous (>1 assignment) — errors with candidate list.
/// 5. No assignment — returns (None, None) so the report proceeds without a milestone.
pub(crate) fn resolve_plan_ctx(
    plan_store: &crate::service::plan::PlanStore,
    pane_id: &str,
    plan_id_hint: Option<&str>,
    task_id_hint: Option<&str>,
    notices: &mut Vec<String>,
) -> Result<(Option<String>, Option<String>), String> {
    let valid_plan = plan_id_hint
        .filter(|s| !s.trim().is_empty())
        .and_then(|id| {
            if plan_store.get(id).is_some() {
                Some(id.to_string())
            } else {
                notices.push(format!(
                    "plan '{}' not found in active plans — ignoring, checking pane assignment",
                    id
                ));
                None
            }
        });

    if valid_plan.is_some() {
        return Ok((valid_plan, task_id_hint.map(str::to_string)));
    }

    let bindings = plan_store.task_bindings_with_name_for_pane(pane_id);
    match bindings.len() {
        0 => Ok((None, None)),
        1 => {
            let (p, t, _name) = bindings.into_iter().next().unwrap();
            Ok((Some(p), Some(t)))
        }
        _ => {
            let list = bindings
                .iter()
                .map(|(p, t, n)| format!("  {t} «{n}» in plan {p}"))
                .collect::<Vec<_>>()
                .join("\n");
            Err(format!(
                "ambiguous: pane {} is assigned to {} tasks — pass --plan and --task to disambiguate:\n{}",
                pane_id,
                bindings.len(),
                list
            ))
        }
    }
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
    let target = target?;
    let storage_key = target.storage_key();
    if report_store.has_report_from(&storage_key, pane_id) {
        return None;
    }
    Some((
        storage_key,
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
mod tests;
