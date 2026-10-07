use crate::multiplexer::types::ManagedTab;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagedSendOrigin {
    User,
    Agent,
    System,
    Stop,
}

pub fn managed_tab_at_spawn(
    tool_less: bool,
    spawner: Option<(&str, &str, bool)>,
) -> Option<ManagedTab> {
    let (id, name, is_agent) = spawner?;
    (tool_less && is_agent && !id.is_empty()).then(|| ManagedTab {
        manager_tab_id: id.to_string(),
        manager_name: name.to_string(),
        manager_closed: false,
    })
}

pub fn reconcile_manager(managed: &mut ManagedTab, manager_open: bool) {
    if !manager_open {
        managed.manager_closed = true;
    }
}

pub fn managed_send_refusal(
    managed: Option<&ManagedTab>,
    origin: ManagedSendOrigin,
) -> Option<String> {
    let managed = managed.filter(|state| state.is_active())?;
    (origin == ManagedSendOrigin::User).then(|| {
        format!("This tab is managed by {}. Only agent tabs can send messages; you can stop generation.", managed.manager_name)
    })
}

#[cfg(test)]
mod tests;
