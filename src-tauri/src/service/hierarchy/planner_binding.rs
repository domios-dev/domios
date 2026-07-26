use crate::service::plan::PlanStore;

pub(crate) fn clear_planner_bindings_for_lifecycle_exit(
    plan_store: &PlanStore,
    pane_id: &str,
    now_ms: u64,
) -> usize {
    let bindings: Vec<_> = plan_store
        .list_full()
        .into_iter()
        .filter_map(|plan| {
            let Some(crate::plan::PlannerRef::Pane {
                pane_id: bound,
                session_id: Some(_),
                title,
            }) = &plan.planner
            else {
                return None;
            };
            (bound == pane_id).then(|| (plan.id, title.clone()))
        })
        .collect();
    bindings
        .into_iter()
        .filter(|(plan_id, title)| {
            plan_store
                .set_planner_ref(
                    plan_id,
                    Some(crate::plan::PlannerRef::Pane {
                        pane_id: pane_id.to_string(),
                        session_id: None,
                        title: title.clone(),
                    }),
                    now_ms,
                )
                .is_ok()
        })
        .count()
}
