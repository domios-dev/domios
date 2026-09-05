use crate::plan::{DecisionGate, PlannerRef};

impl crate::service::ServiceContext {
    pub(crate) fn planner_gate_owner_answerer(
        &self,
        plan_id: &str,
        asked_by_tab_id: Option<&str>,
    ) -> Option<String> {
        let plan = self.plan_store.get(plan_id)?;
        planner_owner_for_gate(&plan, asked_by_tab_id).map(str::to_string)
    }

    pub(crate) fn deliver_planner_gate_to_owner(
        &self,
        plan_id: &str,
        requested_owner: &str,
        gate: &DecisionGate,
    ) -> Result<(), String> {
        let plan = self.plan_store.get(plan_id).ok_or("plan not found")?;
        let intended_owner = plan
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
        let envelope = crate::service::messenger::MessageEnvelope::codeterm_reminder("plan");
        crate::service::orchestrator_digests::deliver_exact_role_target(
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
                .map(|_| ())
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::plan::{OrchestratorRef, Plan, PlannerRef};

    #[test]
    fn planner_gate_routes_to_the_plan_owner() {
        let mut plan = Plan::new(
            "routing".into(),
            "cwd".into(),
            OrchestratorRef::from_tab("owner"),
        );
        plan.planner = Some(PlannerRef::Tab {
            tab_id: "planner".into(),
            session_id: Some("session".into()),
            title: None,
        });

        assert_eq!(
            super::planner_owner_for_gate(&plan, Some("planner")),
            Some("owner")
        );
        assert_eq!(super::planner_owner_for_gate(&plan, Some("worker")), None);
    }
}

pub(super) fn planner_owner_for_gate<'a>(
    plan: &'a crate::plan::Plan,
    asked_by_tab_id: Option<&str>,
) -> Option<&'a str> {
    let PlannerRef::Tab { tab_id, .. } = plan.planner.as_ref()? else {
        return None;
    };
    (asked_by_tab_id == Some(tab_id.as_str()))
        .then(|| plan.owner.tab_id.as_deref())
        .flatten()
}
