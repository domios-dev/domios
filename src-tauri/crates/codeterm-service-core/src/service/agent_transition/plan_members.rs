use super::*;

pub(super) fn relink_plan_members(
    mux: &mut MultiplexerState,
    plan_store: &crate::service::plan::PlanStore,
    rebind: Option<&AgentLifecyclePlanRebind>,
) -> Result<(), String> {
    let Some(rebind) = rebind else {
        return Ok(());
    };
    let owner_tab_id = rebind
        .owner
        .tab_id
        .as_deref()
        .ok_or_else(|| "plan rebind requires a destination Orchestrator tab".to_string())?;
    let mut targets = plan_store
        .relink_targets_projection(&rebind.plan_id, false)
        .map_err(|error| error.to_string())?;
    for tab in mux.all_tabs().filter(|tab| {
        tab.role == Some(AgentRole::Worker)
            && tab
                .task_binding
                .as_ref()
                .and_then(|binding| binding.plan_id.as_deref())
                == Some(rebind.plan_id.as_str())
    }) {
        if !targets.contains(&tab.id) {
            targets.push(tab.id.clone());
        }
    }
    for tab_id in targets {
        if tab_id == owner_tab_id {
            continue;
        }
        let Some(tab) = mux.get_tab(&tab_id) else {
            continue;
        };
        if tab.role != Some(AgentRole::Worker) {
            continue;
        }
        if tab
            .task_binding
            .as_ref()
            .and_then(|binding| binding.plan_id.as_deref())
            .is_some_and(|plan_id| plan_id != rebind.plan_id)
        {
            return Err(format!("Worker {tab_id} is bound to a different plan"));
        }
        if tab.report_target_mesh_peer.is_some() || tab.ownership.is_some() {
            return Err(format!(
                "Plan {} has remote Worker {tab_id}; transfer on its owning machine",
                rebind.plan_id
            ));
        }
        mux.repoint_parent(&tab_id, owner_tab_id);
        if let Some(tab) = mux.get_tab_mut(&tab_id) {
            tab.team_id = Some(rebind.team_id.clone());
            tab.report_target_tab_id = Some(owner_tab_id.to_string());
        }
    }
    Ok(())
}
