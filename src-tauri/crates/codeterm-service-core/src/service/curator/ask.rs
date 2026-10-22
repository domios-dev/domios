use super::ServiceContext;

pub(super) fn command(owner: &str) -> String {
    format!("domios send --tab {owner} --from-pad ask.md")
}

pub(super) fn notification_refusal(service: &ServiceContext, tab: &str) -> String {
    let owner = service
        .try_ext::<super::CuratorTabs>()
        .and_then(|index| index.0.read().get(tab).cloned())
        .and_then(|plan| service.plan_store.get(&plan))
        .and_then(|plan| plan.owner.tab_id);
    format!(
        "Curator cannot use notify; ask its Orchestrator with {}",
        command(owner.as_deref().unwrap_or("<Orchestrator-tab-from-digest>"))
    )
}
