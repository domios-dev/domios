use codeterm_service_core::service::ServiceContext;

pub(super) async fn stop_members(
    svc: &ServiceContext,
    member_tab_ids: &[String],
) -> Result<(), String> {
    let runtime_service = svc.clone();
    let runtime_tab_ids = member_tab_ids.to_vec();
    tokio::task::spawn_blocking(move || {
        let mut errors = Vec::new();
        for tab_id in runtime_tab_ids {
            if let Err(error) = runtime_service.shutdown_tab_agent_runtime(&tab_id) {
                errors.push(format!("{tab_id}: {error}"));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "Team runtime shutdown failed: {}",
                errors.join("; ")
            ))
        }
    })
    .await
    .map_err(|error| format!("Team runtime shutdown task failed: {error}"))
    .and_then(|result| result)
}
