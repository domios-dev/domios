use crate::service::plan::PlanStore;

pub(crate) fn clear_planner_bindings_for_lifecycle_exit(
    plan_store: &PlanStore,
    tab_id: &str,
    now_ms: u64,
) -> Result<usize, String> {
    let bindings = plan_store
        .hierarchy_planner_bindings_projection(tab_id, true)
        .map_err(|error| error.to_string())?;
    Ok(bindings
        .into_iter()
        .filter(|(plan_id, title)| {
            plan_store
                .set_planner_ref(
                    plan_id,
                    Some(crate::plan::PlannerRef::Tab {
                        tab_id: tab_id.to_string(),
                        session_id: None,
                        title: title.clone(),
                    }),
                    now_ms,
                )
                .is_ok()
        })
        .count())
}
