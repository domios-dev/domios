use crate::plan::{OrchestratorRef, PlannerRef};
use crate::service::ServiceContext;

/// Bind a pending pair-spawned Planner when an Orchestrator creates its
/// first plan. Returns true only when a pending tab was consumed.
pub fn bind_pending_paired_planner(
    svc: &ServiceContext,
    plan_id: &str,
    owner: &OrchestratorRef,
    now_ms: u64,
) -> Result<bool, String> {
    let Some(orch_tab_id) = owner.tab_id.as_deref().filter(|p| !p.is_empty()) else {
        return Ok(false);
    };
    let planner_tab_id = {
        let mut mux = svc.mux.write();
        let Some(orch) = mux.get_tab_mut(orch_tab_id) else {
            return Ok(false);
        };
        orch.pending_paired_planner_tab_id.take()
    };
    let Some(planner_tab_id) = planner_tab_id else {
        return Ok(false);
    };
    let (session_id, title) = svc
        .mux
        .read()
        .get_tab(&planner_tab_id)
        .map(|planner| {
            let binding_is_current =
                planner
                    .session_binding_nonce
                    .as_deref()
                    .is_some_and(|nonce| {
                        planner.reported_session_binding_nonce.as_deref() == Some(nonce)
                    });
            (
                binding_is_current
                    .then(|| planner.agent_session_id.clone())
                    .flatten(),
                Some(planner.title.clone()),
            )
        })
        .unwrap_or_default();
    svc.plan_store
        .set_planner_ref(
            plan_id,
            Some(PlannerRef::Tab {
                tab_id: planner_tab_id.clone(),
                session_id,
                title,
            }),
            now_ms,
        )
        .map_err(|e| e.to_string())?;
    log::info!(
        "[orchestration-pair] bound pending planner {planner_tab_id} to first plan {plan_id} for orchestrator {orch_tab_id}"
    );
    Ok(true)
}

pub fn tab_has_pending_paired_planner(svc: &ServiceContext, tab_id: &str) -> bool {
    svc.mux
        .read()
        .get_tab(tab_id)
        .and_then(|p| p.pending_paired_planner_tab_id.as_deref())
        .map(|p| !p.is_empty())
        .unwrap_or(false)
}
