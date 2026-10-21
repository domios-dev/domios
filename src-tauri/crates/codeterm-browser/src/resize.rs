use std::sync::Arc;

use parking_lot::Mutex;

use crate::runtime::{self, Runtime};
use crate::screencast::{FrameHub, ScreencastParams};
use crate::viewport::{ResizeModel, ViewportModel};

#[derive(Default)]
pub(super) struct Coordinator {
    session: String,
    model: ResizeModel,
}

pub(super) async fn apply(
    runtime: Arc<Runtime>,
    current: Arc<Mutex<Option<Arc<Runtime>>>>,
    viewport: Arc<Mutex<ViewportModel>>,
    frames: Arc<FrameHub>,
    coordinator: Arc<tokio::sync::Mutex<Coordinator>>,
) -> Result<(), String> {
    // HTTP disconnects must not cancel a CDP command before its acknowledgement.
    tokio::spawn(async move {
        let mut coordinator = coordinator.lock().await;
        if coordinator.session != runtime.page.session_id() {
            coordinator.session = runtime.page.session_id().to_owned();
            coordinator.model = ResizeModel::default();
        }
        loop {
            let still_current = current
                .lock()
                .as_ref()
                .is_some_and(|page| Arc::ptr_eq(page, &runtime));
            if !still_current {
                return Ok(());
            }
            let latest = viewport.lock().current();
            let Some(next) = coordinator.model.begin(latest) else {
                return Ok(());
            };
            let result =
                runtime::apply_viewport(&runtime.client, &runtime.page, runtime.window_id, next)
                    .await;
            coordinator.model.acknowledge(result.is_ok());
            result?;
            {
                let current = current.lock();
                if current
                    .as_ref()
                    .is_some_and(|page| Arc::ptr_eq(page, &runtime))
                {
                    frames.set_params(ScreencastParams::new(next));
                }
            }
        }
    })
    .await
    .map_err(|error| format!("browser resize task failed: {error}"))?
}
