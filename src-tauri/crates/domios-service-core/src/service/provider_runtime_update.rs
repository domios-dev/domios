use super::{agent_runtime, ServiceContext};

pub(crate) fn completed(svc: &ServiceContext, provider: &str, old: &str, new: &str) {
    if old == new || !agent_runtime::engine::requested(svc, provider) {
        return;
    }
    let tabs = svc
        .mux
        .read()
        .all_tabs()
        .filter(|tab| {
            tab.agent_type.as_deref() == Some(provider)
                && matches!(
                    tab.backing,
                    crate::multiplexer::types::TabBacking::ChatBackend { .. }
                )
                && !tab.frozen
        })
        .map(|tab| tab.id.clone())
        .collect::<Vec<_>>();
    for tab in tabs {
        agent_runtime::update::schedule(svc, &tab, new);
    }
}
