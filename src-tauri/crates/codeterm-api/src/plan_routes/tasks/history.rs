use super::*;

#[derive(Deserialize)]
pub struct TaskHistoryQuery {
    kind: codeterm_service_core::service::plan::task_history::TaskHistoryKind,
    before: Option<i64>,
    limit: Option<usize>,
}

pub async fn task_history(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path((id, tid)): Path<(String, String)>,
    Query(query): Query<TaskHistoryQuery>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        task_history_core(
            &state.svc,
            &id,
            &tid,
            query.kind,
            query.before,
            query.limit.unwrap_or(50),
        )
    })
    .await
}

pub(super) fn task_history_core(
    svc: &codeterm_service_core::service::ServiceContext,
    id: &str,
    tid: &str,
    kind: codeterm_service_core::service::plan::task_history::TaskHistoryKind,
    before: Option<i64>,
    limit: usize,
) -> PlanRouteResult {
    codeterm_orchestration::validate_plan_team_binding(svc, id)
        .map_err(task_board_team_plan_err)?;
    svc.plan_store
        .task_history_page(id, tid, kind, before, limit)
        .map(Json)
        .map_err(plan_err)
}
