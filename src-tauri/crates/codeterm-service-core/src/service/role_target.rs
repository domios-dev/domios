pub(crate) fn deliver_exact_role_target(
    role: &str,
    plan_id: &str,
    intended_target: &str,
    requested_target: &str,
    deliver: impl FnOnce(&str) -> Result<crate::service::messenger::Delivery, String>,
) -> Result<crate::service::messenger::Delivery, String> {
    if intended_target != requested_target {
        log::warn!(
            "[plan-role-delivery] plan={} role={} intended_target={} actual_target=none outcome=refused requested_target={}",
            plan_id,
            role,
            intended_target,
            requested_target
        );
        return Err(format!(
            "{role} payload target mismatch: intended {intended_target}, requested {requested_target}"
        ));
    }
    let result = deliver(intended_target);
    match &result {
        Ok(crate::service::messenger::Delivery::Pending { delivery_id }) => log::warn!(
            "[plan-role-delivery] plan={} role={} intended_target={} actual_target={} outcome=pending delivery_id={}",
            plan_id,
            role,
            intended_target,
            intended_target,
            delivery_id
        ),
        Ok(_) => log::info!(
            "[plan-role-delivery] plan={} role={} intended_target={} actual_target={} outcome=written",
            plan_id,
            role,
            intended_target,
            intended_target
        ),
        Err(error) => log::warn!(
            "[plan-role-delivery] plan={} role={} intended_target={} actual_target={} outcome=failed error={}",
            plan_id,
            role,
            intended_target,
            intended_target,
            error
        ),
    }
    result
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::deliver_exact_role_target;

    #[test]
    fn owner_undeliverable_heartbeat_never_reaches_planner() {
        let attempts = RefCell::new(Vec::new());
        let result = deliver_exact_role_target(
            "orchestrator",
            "plan-one",
            "owner-tab",
            "owner-tab",
            |target| {
                attempts.borrow_mut().push(target.to_string());
                Err("owner undeliverable".to_string())
            },
        );

        assert!(result.is_err());
        assert_eq!(attempts.into_inner(), vec!["owner-tab"]);
    }

    #[test]
    fn owner_heartbeat_refuses_a_planner_target_before_delivery() {
        let attempts = RefCell::new(Vec::new());
        let result = deliver_exact_role_target(
            "orchestrator",
            "plan-one",
            "owner-tab",
            "planner-tab",
            |target| {
                attempts.borrow_mut().push(target.to_string());
                Ok(crate::service::messenger::Delivery::Pty { text: "ok".into() })
            },
        );

        assert!(result.is_err());
        assert!(attempts.into_inner().is_empty());
    }
}
