use crate::traits::*;
pub(super) use codeterm_service_core::service::orchestration_data::epoch_ms;
use codeterm_service_core::service::ServiceContext;
pub struct PlannerDeliveryBinding {
    pub(super) session_id: String,
    pub(super) binding_nonce: String,
    pub(super) transport: codeterm_service_core::plan::PlannerDeliveryTransport,
}

pub(super) const PLANNER_REHYDRATE_MAX_BYTES: usize = 4096;
pub(super) const PLANNER_REHYDRATE_HEADER: &str = "[planner rehydrate]\n";
const PLANNER_REHYDRATE_GATE_LIMIT: usize = 10;
const PLANNER_REHYDRATE_FIELD_BYTES: usize = 64;
const PLANNER_REHYDRATE_QUESTION_BYTES: usize = 192;

impl HierarchyPlannerDeliveryExt for ServiceContext {
    fn bind_planner_session(&self, tab_id: &str) -> Result<(), String> {
        let binding = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return Ok(());
            };
            if tab.session_binding_nonce.is_none()
                || tab.session_binding_nonce != tab.reported_session_binding_nonce
            {
                return Ok(());
            }
            tab.agent_session_id
                .clone()
                .zip(tab.reported_session_binding_nonce.clone())
        };
        let Some((session_id, binding_nonce)) = binding else {
            return Ok(());
        };
        let now_ms = epoch_ms();
        let plans = self
            .plan_store
            .hierarchy_planner_bindings_projection(tab_id, false)
            .map_err(|error| error.to_string())?;
        for (plan_id, _) in plans {
            let _ = self.plan_store.rebind_planner_session(
                &plan_id,
                tab_id,
                &session_id,
                &binding_nonce,
                now_ms,
            );
        }
        Ok(())
    }

    fn prepare_planner_delivery_binding(
        &self,
        tab_id: &str,
    ) -> Result<PlannerDeliveryBinding, String> {
        let is_chatbackend = matches!(
            self.mux.read().get_tab(tab_id).map(|tab| &tab.backing),
            Some(codeterm_service_core::multiplexer::types::TabBacking::ChatBackend { .. })
        );
        if is_chatbackend {
            self.wake_frozen_tab(tab_id)?;
            self.chatbackend_reopen_if_restored(tab_id)?;
        }
        self.bind_planner_session(tab_id)?;
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
                codeterm_service_core::multiplexer::types::TabBacking::Pty => {
                    codeterm_service_core::plan::PlannerDeliveryTransport::PtyAgent
                }
                codeterm_service_core::multiplexer::types::TabBacking::ChatBackend { .. } => {
                    codeterm_service_core::plan::PlannerDeliveryTransport::ChatBackend
                }
                codeterm_service_core::multiplexer::types::TabBacking::Surface(_) => {
                    return Err("agent_unavailable: planner Tab is a surface".into());
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

    fn observed_planner_identity(&self, tab_id: &str) -> (Option<String>, Option<String>) {
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

pub(super) fn planner_rehydrate_payload(
    plan: &codeterm_service_core::service::plan::PlannerRehydrateRead,
) -> String {
    let open = &plan.gates;
    let mut out = format!(
        "{PLANNER_REHYDRATE_HEADER}Plan {} has {} open decision gate(s):\n",
        truncate_utf8(&plan.id, PLANNER_REHYDRATE_FIELD_BYTES),
        plan.open_count
    );
    for gate in open.iter().take(PLANNER_REHYDRATE_GATE_LIMIT) {
        let status = match gate.status {
            codeterm_service_core::plan::GateStatus::Open => "open",
            codeterm_service_core::plan::GateStatus::Escalated => "escalated",
            codeterm_service_core::plan::GateStatus::Answered => "answered",
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
    if plan.open_count > PLANNER_REHYDRATE_GATE_LIMIT {
        out.push_str(&format!(
            "- … {} more open gate(s) omitted from this bounded recovery summary.\n",
            plan.open_count - PLANNER_REHYDRATE_GATE_LIMIT
        ));
    }
    out.push_str(&format!(
        "Full gate records: domios plan decision list {} --open",
        truncate_utf8(&plan.id, PLANNER_REHYDRATE_FIELD_BYTES)
    ));
    debug_assert!(out.len() <= PLANNER_REHYDRATE_MAX_BYTES);
    out
}

fn gate_answerer<'a>(
    plan: &'a codeterm_service_core::service::plan::PlannerRehydrateRead,
    gate: &'a codeterm_service_core::service::plan::PlannerRehydrateGateRead,
) -> Option<&'a str> {
    if let Some(owner) =
        codeterm_service_core::service::plan::PlannerGateRead::owner_for_gate_fields(
            &plan.owner,
            plan.planner.as_ref(),
            gate.asked_by_tab_id.as_deref(),
        )
    {
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
    request: &codeterm_service_core::plan::PlannerRequest,
) -> String {
    format!(
        "[planner request {}]\n{}\n\nAfter handling this request, acknowledge it exactly once with:\n\
         domios plan decision ack-request {} {}",
        request.request_id, request.payload, plan_id, request.request_id
    )
}

pub trait HierarchyPlannerDeliveryExt {
    fn bind_planner_session(&self, tab_id: &str) -> Result<(), String>;
    fn prepare_planner_delivery_binding(
        &self,
        tab_id: &str,
    ) -> Result<PlannerDeliveryBinding, String>;
    fn observed_planner_identity(&self, tab_id: &str) -> (Option<String>, Option<String>);
}
