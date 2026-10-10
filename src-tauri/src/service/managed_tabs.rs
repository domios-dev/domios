use crate::multiplexer::types::ManagedTab;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagedSendOrigin {
    User,
    Agent,
    TeamUtility,
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

pub fn managed_send_origin(
    source: Option<(bool, Option<&str>)>,
    manager_team: Option<&str>,
) -> ManagedSendOrigin {
    match source {
        Some((true, _)) => ManagedSendOrigin::Agent,
        Some((false, Some(team))) if !team.is_empty() && manager_team == Some(team) => {
            ManagedSendOrigin::TeamUtility
        }
        _ => ManagedSendOrigin::User,
    }
}

pub fn managed_send_refusal(
    managed: Option<&ManagedTab>,
    origin: ManagedSendOrigin,
) -> Option<String> {
    let managed = managed.filter(|state| state.is_active())?;
    (origin == ManagedSendOrigin::User).then(|| {
        format!("This tab is managed by {}. Only agent tabs and utilities in the manager's Team can send messages; you can stop generation.", managed.manager_name)
    })
}

impl super::ServiceContext {
    pub(crate) fn initialize_managed_tab(
        &self,
        tab_id: &str,
        provider: &str,
        creator: Option<&str>,
    ) {
        let tool_less = self
            .agent_registry
            .with_provider(provider, |provider| provider.is_tool_less())
            .unwrap_or(false);
        let mut mux = self.mux.write();
        let managed = creator.and_then(|id| mux.get_tab(id)).and_then(|manager| {
            managed_tab_at_spawn(
                tool_less,
                Some((
                    &manager.id,
                    &manager.title,
                    manager.agent_type.is_some() || manager.agent_booting,
                )),
            )
        });
        if let Some(tab) = mux.get_tab_mut(tab_id) {
            if tab.managed.is_none() {
                tab.managed = managed;
            }
        }
    }

    pub(crate) fn require_managed_send_allowed(
        &self,
        target: &str,
        source: Option<&str>,
    ) -> Result<(), String> {
        let mux = self.mux.read();
        let managed = mux.get_tab(target).and_then(|tab| tab.managed.as_ref());
        let source = source
            .and_then(|id| mux.resolve_tab_id(id))
            .and_then(|id| mux.get_tab(&id));
        let manager_team = managed
            .and_then(|managed| mux.get_tab(&managed.manager_tab_id))
            .and_then(|manager| manager.team_id.as_deref());
        let origin = managed_send_origin(
            source.map(|tab| {
                (
                    tab.agent_type.is_some() || tab.agent_booting,
                    tab.team_id.as_deref(),
                )
            }),
            manager_team,
        );
        let state = managed.cloned().map(|mut state| {
            let manager_open = mux.get_tab(&state.manager_tab_id).is_some();
            reconcile_manager(&mut state, manager_open);
            state
        });
        managed_send_refusal(state.as_ref(), origin).map_or(Ok(()), Err)
    }

    pub(crate) fn require_managed_terminal_input_allowed(
        &self,
        pty_id: &str,
        bytes: &[u8],
    ) -> Result<(), String> {
        if bytes == b"\x1b" {
            return Ok(());
        }
        let tab_id = self.mux.read().tab_id_for_pty(pty_id);
        if let Some(tab_id) = tab_id {
            self.require_managed_send_allowed(&tab_id, None)?;
        }
        Ok(())
    }

    pub(crate) fn release_managed_tabs(&self, manager_id: &str) {
        let mut mux = self.mux.write();
        let ids = mux.all_tabs().map(|tab| tab.id.clone()).collect::<Vec<_>>();
        for id in ids {
            let Some(tab) = mux.get_tab_mut(&id) else {
                continue;
            };
            if let Some(managed) = &mut tab.managed {
                if managed.manager_tab_id == manager_id {
                    reconcile_manager(managed, false);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
