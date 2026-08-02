//! Agent hierarchy: roles, reports, status queries.

mod first_plan;
mod plan_reminder_nudge;
mod planner_binding;
mod planner_delivery;
mod report_persistence;
mod status;
mod worker_handoff;
pub(crate) use worker_handoff::WorkerTurnSignal;

pub(crate) use first_plan::{FirstPlanCaller, FirstPlanTeamError};
pub(crate) use planner_binding::clear_planner_bindings_for_lifecycle_exit;
pub use status::TabStatusInfo;

use planner_delivery::{
    epoch_ms, planner_rehydrate_payload, planner_request_payload, planner_specs_for_plan,
};
use report_persistence::persist_local_task_report;

use super::{ReportResult, ServiceContext, ServiceState};
use crate::agent::ledger::LedgerRow;
use crate::agent::reports::{ReportDetails, ReportModelIdentity, ReportStatus};
use crate::plan::AgentRef;
use tauri::Manager;

pub(crate) fn start_plan_reminder_scheduler(svc: ServiceState) {
    plan_reminder_nudge::start_plan_reminder_scheduler(svc);
}

impl ServiceContext {
    pub(crate) fn schedule_planner_replay(&self, tab_id: &str) {
        let svc = self.clone();
        let tab_id = tab_id.to_string();
        std::thread::spawn(move || {
            svc.replay_planner_requests_if_routable(&tab_id);
            svc.replay_pending_reports_if_routable(&tab_id);
        });
    }

    fn replay_pending_reports_if_routable(&self, tab_id: &str) {
        let pending = self.report_store.lock().claim_pending_for(tab_id);
        for report in pending {
            let envelope = crate::service::messenger::MessageEnvelope::report(
                &report.from_title,
                &report.from_tab_id,
            )
            .model(report.model.clone());
            match self.gateway_deliver(
                tab_id,
                envelope,
                &report.inject_body(),
                crate::service::message_gateway::DeliveryClass::AgentTraffic,
            ) {
                Ok(delivery) => {
                    self.report_store.lock().mark_delivered(&report.id);
                    log::info!(
                        "[report/replay] target={} report={} delivered via {:?}",
                        tab_id,
                        report.id,
                        delivery
                    );
                }
                Err(error) => {
                    self.report_store.lock().release_delivery(&report.id);
                    log::warn!(
                        "[report/replay] target={} report={} deferred: {}",
                        tab_id,
                        report.id,
                        error
                    );
                }
            }
        }
    }

    pub(crate) fn clear_dead_planner_binding(&self, tab_id: &str) {
        clear_planner_bindings_for_lifecycle_exit(&self.plan_store, tab_id, epoch_ms());
    }

    pub(crate) fn replay_planner_requests_if_routable(&self, tab_id: &str) {
        let now_ms = epoch_ms();
        for plan in self.plan_store.list_full() {
            let Some(crate::plan::PlannerRef::Tab {
                tab_id: bound,
                session_id: Some(session_id),
                ..
            }) = &plan.planner
            else {
                continue;
            };
            if bound != tab_id {
                continue;
            }
            let resolution =
                crate::plan::peer::resolve_planner_ref(&plan.planner, &plan.owner, |id| {
                    self.tab_probe(id)
                });
            if !matches!(
                resolution,
                crate::plan::peer::PeerResolution::Planner { .. }
            ) {
                continue;
            }
            let Some(binding_nonce) = self
                .mux
                .read()
                .get_tab(tab_id)
                .and_then(|tab| tab.reported_session_binding_nonce.clone())
            else {
                continue;
            };
            if plan
                .planner_requests
                .iter()
                .all(|request| matches!(request.state, crate::plan::PlannerRequestState::Completed))
            {
                continue;
            }
            let Ok(requests) = self.plan_store.claim_planner_requests(
                &plan.id,
                tab_id,
                session_id,
                &binding_nonce,
                now_ms,
            ) else {
                continue;
            };
            if requests.is_empty() {
                continue;
            }
            let binding = match self.prepare_planner_delivery_binding(tab_id) {
                Ok(binding) => binding,
                Err(_) => {
                    let (observed_session_id, observed_binding_nonce) =
                        self.observed_planner_identity(tab_id);
                    for request in &requests {
                        let _ = self.plan_store.release_planner_request(
                            &plan.id,
                            &request.request_id,
                            session_id,
                            &binding_nonce,
                            epoch_ms(),
                        );
                        if let (Some(session_id), Some(observed_nonce)) =
                            (&observed_session_id, &observed_binding_nonce)
                        {
                            let _ = self.plan_store.release_planner_request(
                                &plan.id,
                                &request.request_id,
                                session_id,
                                observed_nonce,
                                epoch_ms(),
                            );
                        }
                    }
                    self.clear_dead_planner_binding(tab_id);
                    continue;
                }
            };
            let Some(rebound_plan) = self.plan_store.get(&plan.id) else {
                continue;
            };
            let rehydrate = planner_rehydrate_payload(
                &rebound_plan,
                planner_specs_for_plan(&self.plan_store, &rebound_plan),
            );
            let rehydrate_id = format!("planner-rehydrate-{}-{}", plan.id, binding.session_id);
            if self
                .deliver_to_routable_planner(
                    &plan.id,
                    tab_id,
                    &binding.session_id,
                    &binding.binding_nonce,
                    "planner-recovery",
                    &rehydrate,
                    rehydrate_id,
                )
                .is_err()
            {
                let (observed_session_id, observed_binding_nonce) =
                    self.observed_planner_identity(tab_id);
                for request in &requests {
                    let _ = self.plan_store.release_planner_request(
                        &plan.id,
                        &request.request_id,
                        &binding.session_id,
                        &binding.binding_nonce,
                        epoch_ms(),
                    );
                    if let (Some(session_id), Some(binding_nonce)) =
                        (&observed_session_id, &observed_binding_nonce)
                    {
                        let _ = self.plan_store.release_planner_request(
                            &plan.id,
                            &request.request_id,
                            session_id,
                            binding_nonce,
                            epoch_ms(),
                        );
                    }
                }
                self.clear_dead_planner_binding(tab_id);
                continue;
            }
            for request in requests {
                if self
                    .plan_store
                    .start_planner_request_delivery(
                        &plan.id,
                        &request.request_id,
                        &binding.session_id,
                        &binding.binding_nonce,
                        binding.transport,
                        epoch_ms(),
                    )
                    .is_err()
                {
                    continue;
                }
                let body = planner_request_payload(&plan.id, &request);
                let message_id = format!("planner-request-{}", request.request_id);
                match self.deliver_to_routable_planner(
                    &plan.id,
                    tab_id,
                    &binding.session_id,
                    &binding.binding_nonce,
                    "planner-request",
                    &body,
                    message_id,
                ) {
                    Ok(_) => {
                        let _ = self.plan_store.accept_planner_request(
                            &plan.id,
                            &request.request_id,
                            &binding.session_id,
                            &binding.binding_nonce,
                            epoch_ms(),
                        );
                    }
                    Err(error) => {
                        let (observed_session_id, observed_binding_nonce) =
                            self.observed_planner_identity(tab_id);
                        let _ = self.plan_store.mark_planner_delivery_uncertain(
                            &plan.id,
                            &request.request_id,
                            &error,
                            observed_session_id.as_deref(),
                            observed_binding_nonce.as_deref(),
                            epoch_ms(),
                        );
                        self.clear_dead_planner_binding(tab_id);
                    }
                }
            }
        }
    }

    fn deliver_to_routable_planner(
        &self,
        plan_id: &str,
        tab_id: &str,
        expected_session_id: &str,
        expected_binding_nonce: &str,
        context: &str,
        body: &str,
        message_id: String,
    ) -> Result<(), String> {
        let plan = self.plan_store.get(plan_id).ok_or("plan not found")?;
        let matches_binding = matches!(
            &plan.planner,
            Some(crate::plan::PlannerRef::Tab { tab_id: bound, session_id: Some(session), .. })
                if bound == tab_id && session == expected_session_id
        );
        if !matches_binding
            || !self
                .tab_probe(tab_id)
                .planner_routable_for(Some(expected_session_id))
        {
            return Err("agent_unavailable".into());
        }
        let envelope = crate::service::messenger::MessageEnvelope::codeterm_reminder(context);
        crate::service::messenger::Messenger::deliver_planner_session(
            self,
            tab_id,
            expected_session_id,
            expected_binding_nonce,
            envelope,
            body,
            message_id,
        )
        .map(|_| ())
    }

    pub(crate) fn acknowledge_planner_request(
        &self,
        plan_id: &str,
        request_id: &str,
        tab_id: &str,
    ) -> Result<crate::plan::PlannerRequest, String> {
        let plan = self.plan_store.get(plan_id).ok_or("plan not found")?;
        let request = plan
            .planner_requests
            .iter()
            .find(|request| request.request_id == request_id)
            .cloned()
            .ok_or("planner request not found")?;
        if request.state == crate::plan::PlannerRequestState::Completed {
            return Ok(request);
        }
        let (session_id, binding_nonce) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id).ok_or("agent_unavailable")?;
            (
                tab.agent_session_id.clone().ok_or("agent_unavailable")?,
                tab.reported_session_binding_nonce
                    .clone()
                    .ok_or("agent_unavailable")?,
            )
        };
        let delivered_here = request.delivered_to_tab_id.as_deref() == Some(tab_id)
            && request.delivered_to_session_id.as_deref() == Some(session_id.as_str())
            && request.delivered_with_binding_nonce.as_deref() == Some(binding_nonce.as_str());
        if !matches!(
            request.state,
            crate::plan::PlannerRequestState::InProgress
                | crate::plan::PlannerRequestState::Accepted
        ) || !delivered_here
            || !self
                .tab_probe(tab_id)
                .planner_routable_for(Some(&session_id))
        {
            return Err("agent_unavailable".into());
        }
        self.plan_store
            .complete_planner_request(plan_id, request_id, epoch_ms())
            .map_err(|error| error.to_string())?;
        self.plan_store
            .get(plan_id)
            .and_then(|plan| {
                plan.planner_requests
                    .into_iter()
                    .find(|request| request.request_id == request_id)
            })
            .ok_or_else(|| "planner request not found".into())
    }
    pub fn set_can_spawn(&self, tab_id: &str, can_spawn: bool) -> Result<(), String> {
        self.mux.write().set_can_spawn(tab_id, can_spawn)?;
        self.emit_state_changed();
        Ok(())
    }

    /// Post-condition for spawn: `--parent` on a Worker must land on the tab.
    pub(crate) fn expect_spawn_parent_link(
        &self,
        tab_id: &str,
        expected_parent: &str,
    ) -> Result<(), String> {
        let mux = self.mux.read();
        let tab = mux
            .get_tab(tab_id)
            .ok_or_else(|| format!("Tab {} not found after spawn role apply", tab_id))?;
        match tab.parent_tab_id.as_deref() {
            Some(p) if p == expected_parent => Ok(()),
            Some(p) => Err(format!(
                "spawn parent link mismatch on {tab_id}: expected {expected_parent}, got {p}"
            )),
            None => Err(format!(
                "spawn dropped parent_tab_id on {tab_id}: expected parent {expected_parent}"
            )),
        }
    }

    /// Send report to the resolved target. Routing priority:
    /// remote mesh target (`report_target_mesh_peer` + tab/alias) →
    /// `parent_tab_id` → `report_target_tab_id` → `spawned_by_tab_id`.
    /// NEVER routes back to the reporter's own tab; errors when none resolve.
    ///
    /// Plan/task resolution order:
    /// 1. Explicit `plan_id`/`task_id` from the caller — validated against live store.
    /// 2. If provided ids are stale (plan not found) — falls back to tab assignment.
    /// 3. Daemon-side tab assignment lookup (`task_bindings_with_name_for_tab`).
    /// 4. Ambiguous (>1 assignment) — errors with candidate list.
    /// 5. No assignment — proceeds without a plan milestone.
    pub fn submit_report(
        &self,
        tab_id: &str,
        message: &str,
        details: ReportDetails,
        plan_id: Option<&str>,
        task_id: Option<&str>,
        milestone_kind: Option<&str>,
    ) -> Result<ReportResult, String> {
        let task_status = report_task_status_transition(details.status)?;
        let mut report_requirement = worker_handoff::claim_explicit_report(&self.gateway, tab_id);

        let mut notices: Vec<String> = Vec::new();
        let (eff_plan_id, eff_task_id) =
            resolve_plan_ctx(&self.plan_store, tab_id, plan_id, task_id, &mut notices)?;
        let mut persisted_task = None;

        let (from_title, default_target_opt, model_identity, from_ref) = {
            let mux = self.mux.read();
            let (from_title, target_opt) = match resolve_report_target(&mux, tab_id) {
                Ok(resolved) => resolved,
                Err(_) if eff_plan_id.is_some() => (tab_id.to_string(), None),
                Err(e) => return Err(e),
            };
            let tab = mux.get_tab(tab_id);
            let model_identity = tab.map(|p| ReportModelIdentity {
                actual_model: p.agent_session_model.clone(),
                planned_model: p.agent_planned_model.clone(),
                requested_model: p.agent_requested_model.clone(),
                provider_resolved_model: p.agent_provider_resolved_model.clone(),
                fallback: p.model_fallback.clone(),
                mismatch: p.model_identity_mismatch.clone(),
                reasoning_effort: p.reasoning_effort_identity.clone(),
            });
            let from_ref = tab.map_or_else(
                || AgentRef::from_tab(tab_id),
                |p| AgentRef {
                    tab_id: Some(tab_id.to_string()),
                    session_id: p.agent_session_id.clone(),
                    title: Some(p.title.clone()),
                },
            );
            (from_title, target_opt, model_identity, from_ref)
        };
        let model = model_identity
            .as_ref()
            .and_then(|identity| identity.actual_model.clone());
        // Capture mesh peer before default_target_opt is consumed by .or().
        let default_mesh_peer = match &default_target_opt {
            Some(ReportTarget::Mesh { peer, .. }) => Some(peer.clone()),
            _ => None,
        };

        let target_opt = eff_plan_id
            .as_deref()
            .and_then(|id| self.plan_store.owner_of(id))
            .and_then(|owner| owner.tab_id)
            .filter(|target| !target.is_empty() && target != tab_id)
            .map(ReportTarget::Local)
            .or(default_target_opt);
        let target = target_opt.ok_or_else(|| {
            "No report target for this tab. You are not attached to an orchestrator, report target, or spawning tab. Ask for the target tab id, then use `codeterm send [--mesh <peer>] --tab <tab-or-alias> \"...\"`, or spawn with a report target.".to_string()
        })?;

        let foreign = is_explicit_plan_foreign(plan_id, eff_plan_id.as_deref());
        let homed_to_remote = if eff_plan_id.is_none() || foreign {
            let h_plan = plan_id.map(str::trim).filter(|s| !s.is_empty());
            let h_task = task_id.map(str::trim).filter(|s| !s.is_empty());
            match (h_plan, h_task, &default_mesh_peer) {
                (Some(h_plan), Some(h_task), Some(peer)) => {
                    if let Err(e) = self.relay_plan_batch_to_peer(
                        peer.clone(),
                        h_plan.to_string(),
                        h_task.to_string(),
                        from_ref.clone(),
                        task_status,
                        milestone_text(message, details.status),
                        milestone_kind.unwrap_or("status_update").to_string(),
                    ) {
                        notices.push(format!(
                            "plan milestone relay to peer '{}' failed — check mesh pairing with \
                             `codeterm mesh status`, then retry: {}",
                            peer, e
                        ));
                    }
                    if !details.attachments.is_empty() {
                        notices.push(
                            "report attachments were not relayed to the remote owning daemon; \
                             mesh plan relay currently carries text/status only"
                                .into(),
                        );
                    }
                    true
                }
                (Some(h_plan), Some(_), None) if foreign => {
                    // Explicit foreign plan but no mesh peer on this tab.
                    notices.push(format!(
                        "plan '{}' is not active on this machine and no mesh homing peer is \
                         configured; milestone NOT recorded — spawn this worker via \
                         `codeterm agent spawn --mesh <peer>` so the report homes to the \
                         plan's owning machine",
                        h_plan
                    ));
                    false
                }
                _ => false,
            }
        } else {
            false
        };

        if let Some(ref plan_id) = eff_plan_id {
            if !homed_to_remote && !foreign {
                let task_id = eff_task_id.as_deref().ok_or_else(|| {
                    report_persistence::report_persistence_incident(
                        tab_id,
                        plan_id,
                        None,
                        "the report resolved a plan but no exact task",
                    )
                })?;
                persist_local_task_report(
                    &self.plan_store,
                    tab_id,
                    plan_id,
                    task_id,
                    &from_ref,
                    message,
                    &details,
                    task_status,
                    milestone_kind,
                    epoch_ms(),
                )?;
                persisted_task = Some((plan_id.clone(), task_id.to_string()));
                self.plan_turn_counter.reset_activity(tab_id);
                crate::api::events::emit_everywhere(
                    &self.app_handle,
                    "plan-changed",
                    serde_json::json!({ "plan_id": plan_id }),
                );
            }
        }

        let storage_key = target.storage_key();
        let report = self.report_store.lock().add_detailed_with_model_identity(
            &storage_key,
            tab_id,
            &from_title,
            message,
            details,
            model_identity,
        );
        self.report_store.lock().claim(&report.id);
        report_requirement.accept();

        self.events.broadcast(
            "agent.report",
            serde_json::json!({
                "target_tab_id": storage_key,
                "report": report.clone(),
            }),
        );
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "agent-report",
            serde_json::json!({
                "target_tab_id": storage_key,
                "from_tab_id": tab_id,
                "from_title": from_title,
                "message": message,
                "report": report.clone(),
            }),
        );

        match target {
            ReportTarget::Mesh { peer, tab } => {
                self.report_store.lock().mark_delivered(&report.id);
                self.relay_report_to_peer(
                    peer,
                    tab,
                    from_title,
                    tab_id.to_string(),
                    model.clone(),
                    report.inject_body(),
                );
            }
            ReportTarget::Local(target_id) => {
                let envelope =
                    crate::service::messenger::MessageEnvelope::report(&from_title, tab_id)
                        .model(model.clone());
                // The target is a TUI that owns its screen: it reflows, collapses
                // long input to a paste placeholder, and queues while busy. Echo
                // scraping cannot decide delivery there, and its false negatives
                // failed reports that had already landed. The report is stored and
                // broadcast above; the write is the guarantee.
                match self.gateway_deliver(
                    &target_id,
                    envelope,
                    &report.inject_body(),
                    crate::service::message_gateway::DeliveryClass::AgentTraffic,
                ) {
                    Ok(delivery) => {
                        self.report_store.lock().mark_delivered(&report.id);
                        log::info!("[report] target={} delivered via {:?}", target_id, delivery)
                    }
                    Err(error) => {
                        self.report_store.lock().release_delivery(&report.id);
                        log::warn!("[report] target={} deliver: {}", target_id, error);
                    }
                }
            }
        }

        self.record_kpi_report(
            tab_id,
            eff_plan_id.as_deref(),
            eff_task_id.as_deref(),
            model.as_deref(),
            message.len(),
            &report.id,
        );

        Ok(ReportResult {
            report_id: report.id,
            orchestrator_tab_id: storage_key,
            notices,
            persisted_task,
        })
    }

    /// §EFFORT: moving a task to `in_review` IS the trigger — the plan system itself
    /// notifies the plan owner (orchestrator) so a worker need not send a separate
    /// `agent report`. The status change carries the intent; this delivers it. Best-effort.
    pub fn notify_owner_task_in_review(
        &self,
        plan_id: &str,
        task_id: &str,
        by_tab: Option<&str>,
        intent: Option<&str>,
    ) {
        let Some(plan) = self.plan_store.get(plan_id) else {
            return;
        };
        let Some(owner) = plan.owner.tab_id.as_deref() else {
            return;
        };
        if Some(owner) == by_tab {
            return; // no self-notify
        }
        let task_name = plan
            .tasks
            .iter()
            .find(|t| t.id == task_id)
            .map(|t| t.name.clone())
            .unwrap_or_else(|| task_id.to_string());
        let who = by_tab.unwrap_or("a worker");
        let intent_part = intent
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| format!(" — {s}"))
            .unwrap_or_default();
        let text = format!(
            "[task in_review] '{task_name}' ({task_id}) → in_review by {who}{intent_part}\nReview it, then judge done or bounce back."
        );
        let text = crate::service::orchestrator_digests::prepend_orchestrator_heartbeat(
            self,
            plan_id,
            Some(owner),
            &text,
        );
        let envelope = crate::service::messenger::MessageEnvelope::codeterm_reminder("plan");
        let _ = self.gateway_deliver(
            owner,
            envelope,
            &text,
            crate::service::message_gateway::DeliveryClass::SystemNotice,
        );
    }

    /// Nudge the gate asker tab when a gate is answered.
    pub fn nudge_gate_asker(
        &self,
        plan_id: &str,
        gate_id: &str,
        answer_text: &str,
        answered_by_tab: Option<&str>,
    ) {
        let Some(plan) = self.plan_store.get(plan_id) else {
            return;
        };
        let gate = plan.gates.iter().find(|g| g.id == gate_id);
        let asker_tab = gate
            .and_then(|g| g.asked_by_tab_id.as_deref())
            .or(plan.owner.tab_id.as_deref());
        let Some(asker) = asker_tab else {
            return;
        };
        if Some(asker) == answered_by_tab {
            return; // no self-notify
        }
        let preview = answer_text.chars().take(160).collect::<String>();
        let text = format!(
            "[gate answered: {gate_id}] Answer: {preview}\n\
             Unblocked tasks are now pending. Review with: codeterm plan state {plan_id}"
        );
        let text = crate::service::orchestrator_digests::prepend_orchestrator_heartbeat(
            self,
            plan_id,
            Some(asker),
            &text,
        );
        let envelope = crate::service::messenger::MessageEnvelope::codeterm_reminder("plan");
        let _ = self.gateway_deliver(
            asker,
            envelope,
            &text,
            crate::service::message_gateway::DeliveryClass::SystemNotice,
        );
    }

    /// Relay plan batch ops (status update + milestone) to the peer that owns the
    /// plan. Used when the plan doesn't exist in the local store but plan_id +
    /// task_id were provided (remote worker mesh homing scenario).
    ///
    /// Returns `Err` immediately when the named peer is not in the paired set so
    /// the caller can surface a retry hint. The actual HTTP send is async
    /// fire-and-forget; transport errors are logged but not returned.
    fn relay_plan_batch_to_peer(
        &self,
        peer: String,
        plan_id: String,
        task_id: String,
        from_ref: AgentRef,
        task_status: Option<&'static str>,
        ms_text: String,
        ms_kind: String,
    ) -> Result<(), String> {
        let client = {
            let mesh_handle = self.app_handle.state::<crate::MeshStateHandle>();
            let mesh = mesh_handle.lock();
            resolve_peer_client_by_name(&mesh, &peer)
        };
        let Some(client) = client else {
            return Err(format!(
                "peer '{}' is not paired — run `codeterm mesh status` to check connectivity, \
                 then retry",
                peer
            ));
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
            delivery_id: None,
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
        Ok(())
    }

    /// Fire-and-forget delivery of a report to a tab (id or `main`/`general`
    /// alias) on a paired mesh peer. Resolves the peer by instance name or id,
    /// then rides the same signed relay the CLI `--mesh` send uses; the peer's
    /// relay ingress stamps the authenticated origin so the report renders as
    /// `[Report from <worker>@<peer>]`. Errors are logged, never surfaced — the
    /// report is already stored locally and the reporter must not block.
    fn relay_report_to_peer(
        &self,
        peer: String,
        target_tab: String,
        from_title: String,
        from_tab: String,
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
                from_tab
            );
            return;
        };
        let mut send_body = serde_json::json!({
            "text": body,
            "source_tab": from_tab,
            "source_label": from_title,
            "kind": "report",
            "enter": true,
        });
        if let Some(m) = model.filter(|m| !m.trim().is_empty()) {
            send_body["source_model"] = serde_json::json!(m);
        }
        let cmd = crate::mesh::types::RelayCommand {
            method: "POST".into(),
            path: format!("/api/tabs/{}/send", target_tab),
            body: send_body,
            // At-most-once: a report is one logical send; dedupe by this id so a
            // relay retry can never double-post it into the target tab.
            delivery_id: Some(uuid::Uuid::new_v4().to_string()),
        };
        tauri::async_runtime::spawn(async move {
            match client.send_relay(&cmd).await {
                Ok(r) if r.ok => log::info!("[report/mesh] delivered to {}/{}", peer, target_tab),
                Ok(r) => log::warn!(
                    "[report/mesh] {} rejected report to {}: {:?}",
                    peer,
                    target_tab,
                    r.error
                ),
                Err(e) => log::warn!("[report/mesh] relay to {} failed: {}", peer, e),
            }
        });
    }

    pub fn get_reports(&self, tab_id: &str) -> Vec<serde_json::Value> {
        let reports = self.report_store.lock().get(tab_id, false);
        serde_json::to_value(&reports)
            .unwrap_or_default()
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_and_clear_reports(&self, tab_id: &str) -> Vec<serde_json::Value> {
        let reports = self.report_store.lock().get(tab_id, true);
        serde_json::to_value(&reports)
            .unwrap_or_default()
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    /// Durable per-orchestrator ledger rows (survive restart) from the sqlite ledger,
    /// unlike [`get_reports`](Self::get_reports) which reads the in-memory stream.
    pub fn get_ledger(&self, orchestrator_tab_id: &str) -> Vec<LedgerRow> {
        let rows = self.report_store.lock().ledger_rows(orchestrator_tab_id);
        let mux = self.mux.read();
        ledger_with_live_children(&mux, orchestrator_tab_id, rows, epoch_seconds())
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

fn epoch_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn ledger_with_live_children(
    mux: &crate::multiplexer::state::MultiplexerState,
    orchestrator_tab_id: &str,
    mut rows: Vec<LedgerRow>,
    now: u64,
) -> Vec<LedgerRow> {
    for child_id in mux.get_children_ids(orchestrator_tab_id) {
        let Some(child) = mux.get_tab(&child_id) else {
            continue;
        };
        let title = mux.display_label_for_tab(&child_id);
        match rows.iter_mut().find(|r| r.worker_tab_id == child_id) {
            Some(row) => {
                row.title = title;
                row.model = child
                    .agent_session_model
                    .clone()
                    .or_else(|| row.model.clone());
                row.updated_at = row.updated_at.max(now);
            }
            None => rows.push(LedgerRow {
                orchestrator_tab_id: orchestrator_tab_id.to_string(),
                worker_tab_id: child_id,
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

/// Where a report should be delivered. A target is either a tab on this same
/// daemon, or a tab (id or `main`/`general` alias) on a paired mesh peer — the
/// latter lets a worker spawned via `agent spawn --mesh PEER` report back to the
/// real orchestrator / General target on another machine without the reporter
/// ever naming the peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportTarget {
    /// A tab on this daemon.
    Local(String),
    /// A tab id or alias on a paired peer, addressed by its instance name/id.
    Mesh { peer: String, tab: String },
}

impl ReportTarget {
    /// Stable key the report store and worker-death dedup index by. Local targets
    /// key on the tab id; mesh targets
    /// key on a synthetic `mesh:<peer>/<tab>` so they neither collide with local
    /// tabs nor each other.
    pub fn storage_key(&self) -> String {
        match self {
            ReportTarget::Local(id) => id.clone(),
            ReportTarget::Mesh { peer, tab } => format!("mesh:{}/{}", peer, tab),
        }
    }
}

/// True when the caller passed an explicit plan id that `resolve_plan_ctx` did
/// NOT resolve to the same local plan — i.e., the plan is on a different peer's
/// store, or `resolve_plan_ctx` silently substituted a local binding. When this
/// returns true, report homing should use the explicit hint, NOT the local
/// substitution.
pub(crate) fn is_explicit_plan_foreign(
    explicit_plan_id: Option<&str>,
    eff_plan_id: Option<&str>,
) -> bool {
    explicit_plan_id
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| eff_plan_id.map(str::trim) != Some(p))
        .unwrap_or(false)
}

/// Resolve where a report from `tab_id` should be delivered.
///
/// Returns `(from_title, Some(target))` when a real target exists, or
/// `(from_title, None)` when the report has no implicit recipient and must fail
/// with an explicit `codeterm send` hint. The resolution order is:
///
/// 1. `report_target_mesh_peer` (+ tab/alias) — explicit remote target set by
///    `agent spawn --mesh PEER --report-to ...`; wins so a remote worker reports
///    across the mesh, not into a stale local field.
/// 2. `parent_tab_id` — direct parent in the role hierarchy (cascades up)
/// 3. `report_target_tab_id` — cron / mem worker non-hierarchy target
/// 4. `spawned_by_tab_id` — agent that ran `agent spawn`, even when no
///    role parent (top-level Orchestrator spawned by another agent)
///
/// The reporter's own tab id is filtered out at every local level: a tab must
/// never resolve to itself, otherwise reports get injected into the reporter's
/// own buffer and never reach the spawning agent.
pub fn resolve_report_target(
    mux: &crate::multiplexer::state::MultiplexerState,
    tab_id: &str,
) -> Result<(String, Option<ReportTarget>), String> {
    let tab = mux.get_tab(tab_id).ok_or("Tab not found")?;
    let title = mux.display_label_for_tab(tab_id);

    let nonempty = |s: &Option<String>| {
        s.as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    if let (Some(peer), Some(target_tab)) = (
        nonempty(&tab.report_target_mesh_peer),
        nonempty(&tab.report_target_tab_id),
    ) {
        return Ok((
            title,
            Some(ReportTarget::Mesh {
                peer,
                tab: target_tab,
            }),
        ));
    }

    let local = [
        tab.parent_tab_id.as_deref(),
        tab.report_target_tab_id.as_deref(),
        tab.spawned_by_tab_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .find(|candidate| !candidate.is_empty() && *candidate != tab_id)
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
/// 2. If stale (plan not found), falls back to daemon-side tab assignment.
/// 3. If no `plan_id_hint`, tries tab assignment directly.
/// 4. Ambiguous (>1 assignment) — errors with candidate list.
/// 5. No assignment — returns (None, None) so the report proceeds without a milestone.
pub(crate) fn resolve_plan_ctx(
    plan_store: &crate::service::plan::PlanStore,
    tab_id: &str,
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
                    "plan '{}' not found in active plans — ignoring, checking tab assignment",
                    id
                ));
                None
            }
        });

    if valid_plan.is_some() {
        return Ok((valid_plan, task_id_hint.map(str::to_string)));
    }

    let bindings = plan_store.task_bindings_with_name_for_tab(tab_id);
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
                "ambiguous: tab {} is assigned to {} tasks — pass --plan and --task to disambiguate:\n{}",
                tab_id,
                bindings.len(),
                list
            ))
        }
    }
}

/// Whether a dying leaf tab needs a synthetic death report, and to whom.
/// Returns `(target, message)` when the tab has not reported to that target.
pub fn worker_death_report(
    mux: &crate::multiplexer::state::MultiplexerState,
    report_store: &crate::agent::reports::ReportStore,
    tab_id: &str,
) -> Option<(String, String)> {
    let role_label = match mux.get_tab(tab_id)?.role {
        Some(crate::multiplexer::types::AgentRole::Manager) => "MANAGER",
        Some(crate::multiplexer::types::AgentRole::Worker) => "WORKER",
        _ => "WORKER",
    };
    let (_from_title, target) = resolve_report_target(mux, tab_id).ok()?;
    let target = target?;
    let storage_key = target.storage_key();
    if report_store.has_report_from(&storage_key, tab_id) {
        return None;
    }
    Some((
        storage_key,
        format!("{role_label} {} EXITED without reporting (dead)", tab_id),
    ))
}

#[cfg(test)]
mod tests;
