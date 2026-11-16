use codeterm_service_core::plan::DecisionGate;
use codeterm_service_core::service::ServiceContext;

impl HierarchyGateDeliveryExt for ServiceContext {
    fn deliver_planner_gate_to_owner(
        &self,
        plan_id: &str,
        requested_owner: &str,
        gate: &DecisionGate,
    ) -> Result<(), String> {
        let routing = self
            .plan_store
            .planner_gate_context(plan_id, None)
            .map_err(|error| error.to_string())?
            .ok_or("plan not found")?;
        let intended_owner = routing
            .owner
            .tab_id
            .as_deref()
            .ok_or("plan owner has no local delivery target")?;
        let context = gate.context.trim();
        let context = (!context.is_empty()).then(|| format!("\n{context}"));
        let body = format!(
            "[decision gate {}]\n{}{}\n\nAnswer with:\ncodeterm plan decision answer {} {} --answer \"<your answer>\"",
            gate.id,
            gate.question,
            context.unwrap_or_default(),
            plan_id,
            gate.id
        );
        let mut envelope =
            codeterm_service_core::service::messenger::MessageEnvelope::codeterm_reminder("plan");
        envelope.delivery_observer = Some(
            codeterm_service_core::service::messenger::DeliveryWriteObserver::PlanRole {
                plan_id: plan_id.to_string(),
                role: "owner-decision-gate".into(),
            },
        );
        codeterm_service_core::service::role_target::deliver_exact_role_target(
            "owner-decision-gate",
            plan_id,
            intended_owner,
            requested_owner,
            |target| {
                self.gateway_deliver(
                    target,
                    envelope,
                    &body,
                    codeterm_service_core::service::message_gateway::DeliveryClass::SystemNotice,
                )
            },
        )
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests;

pub trait HierarchyGateDeliveryExt {
    fn deliver_planner_gate_to_owner(
        &self,
        plan_id: &str,
        requested_owner: &str,
        gate: &DecisionGate,
    ) -> Result<(), String>;
}
