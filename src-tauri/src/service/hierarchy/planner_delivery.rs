pub(super) struct PlannerDeliveryBinding {
    pub(super) session_id: String,
    pub(super) binding_nonce: String,
    pub(super) transport: crate::plan::PlannerDeliveryTransport,
}

pub(super) const PLANNER_REHYDRATE_MAX_BYTES: usize = 4096;
pub(super) const PLANNER_REHYDRATE_HEADER: &str = "[planner rehydrate]\n";
const PLANNER_REHYDRATE_GATE_LIMIT: usize = 10;
const PLANNER_REHYDRATE_FIELD_BYTES: usize = 64;
const PLANNER_REHYDRATE_QUESTION_BYTES: usize = 192;

impl crate::service::ServiceContext {
    pub(crate) fn bind_planner_session(&self, tab_id: &str) {
        let binding = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return;
            };
            if tab.session_binding_nonce.is_none()
                || tab.session_binding_nonce != tab.reported_session_binding_nonce
            {
                return;
            }
            tab.agent_session_id
                .clone()
                .zip(tab.reported_session_binding_nonce.clone())
        };
        let Some((session_id, binding_nonce)) = binding else {
            return;
        };
        let now_ms = epoch_ms();
        for plan in self.plan_store.list_full() {
            let Some(crate::plan::PlannerRef::Tab { tab_id: bound, .. }) = &plan.planner else {
                continue;
            };
            if bound != tab_id {
                continue;
            }
            let _ = self.plan_store.rebind_planner_session(
                &plan.id,
                tab_id,
                &session_id,
                &binding_nonce,
                now_ms,
            );
        }
    }

    pub(super) fn prepare_planner_delivery_binding(
        &self,
        tab_id: &str,
    ) -> Result<PlannerDeliveryBinding, String> {
        let is_chatbackend = matches!(
            self.mux.read().get_tab(tab_id).map(|tab| &tab.backing),
            Some(crate::multiplexer::types::TabBacking::ChatBackend { .. })
        );
        if is_chatbackend {
            self.chatbackend_reopen_if_restored(tab_id)?;
        }
        self.bind_planner_session(tab_id);
        let binding = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id).ok_or("agent_unavailable")?;
            if tab.session_binding_nonce.is_none()
                || tab.session_binding_nonce != tab.reported_session_binding_nonce
            {
                return Err("agent_unavailable: planner session nonce is stale".into());
            }
            let session_id = tab
                .agent_session_id
                .clone()
                .ok_or("agent_unavailable: planner session missing")?;
            let binding_nonce = tab
                .reported_session_binding_nonce
                .clone()
                .ok_or("agent_unavailable: planner binding nonce missing")?;
            let transport = match &tab.backing {
                crate::multiplexer::types::TabBacking::Pty => {
                    crate::plan::PlannerDeliveryTransport::PtyAgent
                }
                crate::multiplexer::types::TabBacking::ChatBackend { .. } => {
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
            .tab_probe(tab_id)
            .planner_routable_for(Some(&binding.session_id))
        {
            return Err("agent_unavailable: planner is not routable".into());
        }
        Ok(binding)
    }

    pub(super) fn observed_planner_identity(
        &self,
        tab_id: &str,
    ) -> (Option<String>, Option<String>) {
        let mux = self.mux.read();
        let Some(tab) = mux.get_tab(tab_id) else {
            return (None, None);
        };
        (
            tab.agent_session_id.clone(),
            tab.reported_session_binding_nonce.clone(),
        )
    }
}

pub(super) fn planner_rehydrate_payload(plan: &crate::plan::Plan) -> String {
    let open = plan
        .gates
        .iter()
        .filter(|gate| gate.is_open())
        .collect::<Vec<_>>();
    let mut out = format!(
        "{PLANNER_REHYDRATE_HEADER}Plan {} has {} open decision gate(s):\n",
        truncate_utf8(&plan.id, PLANNER_REHYDRATE_FIELD_BYTES),
        open.len()
    );
    for gate in open.iter().take(PLANNER_REHYDRATE_GATE_LIMIT) {
        let status = match gate.status {
            crate::plan::GateStatus::Open => "open",
            crate::plan::GateStatus::Escalated => "escalated",
            crate::plan::GateStatus::Answered => "answered",
        };
        let answerer = gate_answerer(plan, gate).unwrap_or("unrouted");
        let question = gate
            .question
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&format!(
            "- {} [{status}] answerer: {}\n  Question: {}\n",
            truncate_utf8(&gate.id, PLANNER_REHYDRATE_FIELD_BYTES),
            truncate_utf8(answerer, PLANNER_REHYDRATE_FIELD_BYTES),
            truncate_utf8(&question, PLANNER_REHYDRATE_QUESTION_BYTES),
        ));
    }
    if open.len() > PLANNER_REHYDRATE_GATE_LIMIT {
        out.push_str(&format!(
            "- … {} more open gate(s) omitted from this bounded recovery summary.\n",
            open.len() - PLANNER_REHYDRATE_GATE_LIMIT
        ));
    }
    out.push_str(&format!(
        "Full gate records: codeterm plan decision list {} --open",
        truncate_utf8(&plan.id, PLANNER_REHYDRATE_FIELD_BYTES)
    ));
    debug_assert!(out.len() <= PLANNER_REHYDRATE_MAX_BYTES);
    out
}

fn gate_answerer<'a>(
    plan: &'a crate::plan::Plan,
    gate: &'a crate::plan::DecisionGate,
) -> Option<&'a str> {
    if let Some(owner) = super::gate_delivery::planner_owner_for_gate(
        plan,
        gate.asked_by_tab_id.as_deref(),
    ) {
        return Some(owner);
    }
    gate.routed_to_tab_id.as_deref()
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let suffix = "…";
    let mut end = max_bytes.saturating_sub(suffix.len()).min(value.len());
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}{suffix}", &value[..end])
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
