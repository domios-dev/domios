use crate::plan::DecisionGate;

impl crate::service::ServiceContext {
    pub fn deliver_planner_gate_to_owner(
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
        let mut envelope = crate::service::messenger::MessageEnvelope::codeterm_reminder("plan");
        envelope.delivery_observer =
            Some(crate::service::messenger::DeliveryWriteObserver::PlanRole {
                plan_id: plan_id.to_string(),
                role: "owner-decision-gate".into(),
            });
        crate::service::role_target::deliver_exact_role_target(
            "owner-decision-gate",
            plan_id,
            intended_owner,
            requested_owner,
            |target| {
                self.gateway_deliver(
                    target,
                    envelope,
                    &body,
                    crate::service::message_gateway::DeliveryClass::SystemNotice,
                )
            },
        )
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests;
