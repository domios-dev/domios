use super::ServiceContext;

pub(super) fn command(owner: &str) -> String {
    format!("domios send --tab {owner} --from-pad ask.md")
}

pub(super) fn notification_refusal(service: &ServiceContext, tab: &str) -> String {
    let plan = service
        .try_ext::<super::CuratorTabs>()
        .and_then(|index| index.0.read().get(tab).cloned());
    let owner = match plan
        .map(|id| service.plan_store.plan_identity_projection(&id))
        .transpose()
    {
        Ok(plan) => plan.and_then(|plan| plan.owner.tab_id),
        Err(error) => {
            return format!("Curator cannot use notify; Orchestrator lookup failed: {error}")
        }
    };
    format!(
        "Curator cannot use notify; ask its Orchestrator with {}",
        command(owner.as_deref().unwrap_or("<Orchestrator-tab-from-digest>"))
    )
}
