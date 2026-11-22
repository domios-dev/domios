use codeterm_service_core::service::ServiceContext;
use std::time::Duration;
use tokio::task::JoinSet;

pub(super) async fn stop_members(
    svc: &ServiceContext,
    member_tab_ids: &[String],
) -> Result<(), String> {
    let mut stops = JoinSet::new();
    for tab_id in member_tab_ids.iter().cloned() {
        let service = svc.clone();
        stops.spawn(async move {
            service
                .shutdown_tab_agent_runtime_async(&tab_id)
                .await
                .map_err(|error| format!("{tab_id}: {error}"))
        });
    }
    let mut errors = Vec::new();
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        while let Some(result) = stops.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => errors.push(error),
                Err(error) => errors.push(error.to_string()),
            }
        }
    })
    .await;
    if result.is_err() {
        // A deadline must not cancel in-flight process retirement.
        stops.detach_all();
        errors.push("deadline exceeded while stopping member runtimes".into());
    }
    if errors.is_empty() {
        return Ok(());
    }
    errors.sort();
    Err(format!(
        "Team runtime shutdown failed: {}",
        errors.join("; ")
    ))
}
