pub(super) struct PlannerDeliveryBinding {
    pub(super) session_id: String,
    pub(super) binding_nonce: String,
    pub(super) transport: crate::plan::PlannerDeliveryTransport,
}

impl crate::service::ServiceContext {
    pub(crate) fn bind_planner_session(&self, pane_id: &str) {
        let binding = {
            let mux = self.mux.read();
            let Some(pane) = mux.get_pane(pane_id) else {
                return;
            };
            if pane.session_binding_nonce.is_none()
                || pane.session_binding_nonce != pane.reported_session_binding_nonce
            {
                return;
            }
            pane.agent_session_id
                .clone()
                .zip(pane.reported_session_binding_nonce.clone())
        };
        let Some((session_id, binding_nonce)) = binding else {
            return;
        };
        let now_ms = epoch_ms();
        for plan in self.plan_store.list_full() {
            let Some(crate::plan::PlannerRef::Pane { pane_id: bound, .. }) = &plan.planner else {
                continue;
            };
            if bound != pane_id {
                continue;
            }
            let _ = self.plan_store.rebind_planner_session(
                &plan.id,
                pane_id,
                &session_id,
                &binding_nonce,
                now_ms,
            );
        }
    }

    pub(super) fn prepare_planner_delivery_binding(
        &self,
        pane_id: &str,
    ) -> Result<PlannerDeliveryBinding, String> {
        let is_chatbackend = matches!(
            self.mux.read().get_pane(pane_id).map(|pane| &pane.backing),
            Some(crate::multiplexer::types::PaneBacking::ChatBackend { .. })
        );
        if is_chatbackend {
            self.chatbackend_reopen_if_restored(pane_id)?;
        }
        self.bind_planner_session(pane_id);
        let binding = {
            let mux = self.mux.read();
            let pane = mux.get_pane(pane_id).ok_or("agent_unavailable")?;
            if pane.session_binding_nonce.is_none()
                || pane.session_binding_nonce != pane.reported_session_binding_nonce
            {
                return Err("agent_unavailable: planner session nonce is stale".into());
            }
            let session_id = pane
                .agent_session_id
                .clone()
                .ok_or("agent_unavailable: planner session missing")?;
            let binding_nonce = pane
                .reported_session_binding_nonce
                .clone()
                .ok_or("agent_unavailable: planner binding nonce missing")?;
            let transport = match &pane.backing {
                crate::multiplexer::types::PaneBacking::Pty => {
                    crate::plan::PlannerDeliveryTransport::PtyAgent
                }
                crate::multiplexer::types::PaneBacking::ChatBackend { .. } => {
                    crate::plan::PlannerDeliveryTransport::ChatBackend
                }
            };
            PlannerDeliveryBinding {
                session_id,
                binding_nonce,
                transport,
            }
        };
        if !self
            .pane_probe(pane_id)
            .planner_routable_for(Some(&binding.session_id))
        {
            return Err("agent_unavailable: planner is not routable".into());
        }
        Ok(binding)
    }

    pub(super) fn observed_planner_identity(
        &self,
        pane_id: &str,
    ) -> (Option<String>, Option<String>) {
        let mux = self.mux.read();
        let Some(pane) = mux.get_pane(pane_id) else {
            return (None, None);
        };
        (
            pane.agent_session_id.clone(),
            pane.reported_session_binding_nonce.clone(),
        )
    }
}

pub(super) fn planner_specs_for_plan(
    store: &crate::service::plan::PlanStore,
    plan: &crate::plan::Plan,
) -> Vec<crate::plan::Spec> {
    let mut ids = std::collections::BTreeSet::new();
    ids.extend(plan.spec_refs.iter().cloned());
    ids.extend(plan.spec_links.iter().map(|link| link.spec_id.clone()));
    ids.extend(plan.primary_spec_id.iter().cloned());
    let mut specs: Vec<_> = store
        .list_specs_full()
        .into_iter()
        .filter(|spec| spec.plan_id.as_deref() == Some(plan.id.as_str()) || ids.contains(&spec.id))
        .collect();
    specs.sort_by(|left, right| left.id.cmp(&right.id));
    specs
}

pub(super) fn planner_rehydrate_payload(
    plan: &crate::plan::Plan,
    specs: Vec<crate::plan::Spec>,
) -> String {
    let snapshot = serde_json::json!({
        "plan_id": plan.id,
        "owner": plan.owner,
        "goal": plan.goal,
        "specs": specs,
        "sections": plan.sections,
        "tasks": plan.tasks,
        "strides": plan.strides,
        "gates": plan.gates,
        "inbox": plan.planner_inbox,
        "recent_decisions": plan.gates,
        "plan": plan,
    });
    format!("[planner rehydrate]\n{}", snapshot)
}

pub(super) fn planner_request_payload(
    plan_id: &str,
    request: &crate::plan::PlannerRequest,
) -> String {
    format!(
        "[planner request {}]\n{}\n\nAfter handling this request, acknowledge it exactly once with:\n\
         codeterm plan decision ack-request {} {}",
        request.request_id, request.payload, plan_id, request.request_id
    )
}

pub(super) fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
