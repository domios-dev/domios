use std::{future::Future, sync::Arc};

use codeterm_update::discovery_policy::INTERVAL;

pub async fn run(
    ui: Arc<dyn codeterm_service_core::ports::host::UiEvents>,
    state: codeterm_update::UpdateStateHandle,
    bundles: Option<Arc<codeterm_frontend_bundle::FrontendBundles>>,
) {
    periodic(|| async {
        if let Err(error) = super::check(&ui, &state, bundles.clone(), false).await {
            log::debug!("[update-discovery] check failed: {error}");
        }
    })
    .await;
}

async fn periodic<F, Fut>(mut check: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()>,
{
    let mut timer = tokio::time::interval(INTERVAL);
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        timer.tick().await;
        check().await;
    }
}

#[cfg(test)]
mod tests;
