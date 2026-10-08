use std::sync::Arc;

use tokio::sync::broadcast::error::RecvError;

use super::BrowserRuntime;
use crate::service::browser::viewport::ViewportRequest;
use crate::service::surface::runtime::{SurfaceEvent, SurfaceEvents, SurfaceNotice};

/// State first, the newest frame a late viewer missed, then live events; launching happens in-stream.
pub(super) async fn connect(
    runtime: Arc<BrowserRuntime>,
    viewport: Option<ViewportRequest>,
) -> SurfaceEvents {
    if let Some(viewport) = viewport {
        if let Err(error) = runtime.host.set_viewport(viewport).await {
            runtime
                .shared
                .notice(SurfaceNotice::error(format!("resize failed: {error}")));
        }
    }
    let mut state = runtime.shared.subscribe_state();
    let mut notices = runtime.shared.subscribe_notices();
    let mut host_notices = runtime.host.notices();
    let mut frames = runtime.host.subscribe_screencast();
    let latest = runtime.host.latest_frame();
    Box::pin(async_stream::stream! {
        let current = state.borrow_and_update().clone();
        yield SurfaceEvent::State(current);
        if let Some(frame) = latest {
            yield SurfaceEvent::Frame(frame);
        }
        if let Err(error) = runtime.ensure_page().await {
            yield SurfaceEvent::Notice(SurfaceNotice::error(format!("the browser could not start: {error}")));
        }
        loop {
            let next = tokio::select! {
                changed = state.changed() => match changed {
                    Ok(()) => {
                        let current = state.borrow_and_update().clone();
                        Some(SurfaceEvent::State(current))
                    }
                    Err(_) => None,
                },
                delivery = frames.next() => delivery.map(|delivery| SurfaceEvent::Frame(delivery.frame)),
                notice = notices.recv() => match notice {
                    Ok(notice) => Some(SurfaceEvent::Notice(notice)),
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => None,
                },
                notice = host_notices.recv() => match notice {
                    Ok(text) => Some(SurfaceEvent::Notice(SurfaceNotice { text, tone: None })),
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => None,
                },
            };
            match next {
                Some(event) => yield event,
                None => break,
            }
        }
    })
}
