use std::sync::Arc;

use tokio::sync::broadcast::error::RecvError;

use super::BrowserRuntime;
use crate::surface::runtime::{SurfaceEvent, SurfaceEvents, SurfaceNotice};
use codeterm_browser::screencast::{HubNotice, HubNoticeKind};
use codeterm_browser::viewport::ViewportRequest;

fn host_notice(notice: HubNotice) -> SurfaceNotice {
    match notice.kind {
        HubNoticeKind::Plain => SurfaceNotice::plain(notice.text),
        HubNoticeKind::PreviewPaused => SurfaceNotice::preview_paused(notice.text),
    }
}

/// State first, the newest frame a late viewer missed, then live events; launching happens in-stream.
pub async fn connect(
    runtime: Arc<BrowserRuntime>,
    viewport: Option<ViewportRequest>,
    viewer: Option<String>,
    viewer_target: Option<String>,
) -> SurfaceEvents {
    let lease = if let (Some(viewer), Some(viewport)) = (viewer, viewport) {
        let _guard = runtime.action_gate.lock().await;
        let mut model = runtime.viewport.lock().await;
        let page = runtime.host.page();
        let recursive = super::viewport::recursive_viewer(
            viewer_target.as_deref(),
            page.as_ref().map(|page| page.target_id()),
        );
        let generation = model.join(viewer.clone(), viewport, recursive);
        log::debug!(
            "[browser-viewport] join viewer={viewer} viewport={viewport:?} recursive={recursive} lease={generation:?} owner={:?}",
            model.owner()
        );
        if let Err(error) = runtime.apply_viewer_viewport(&mut model).await {
            runtime
                .shared
                .notice(SurfaceNotice::error(format!("resize failed: {error}")));
        }
        generation.map(|generation| ViewerLease {
            runtime: runtime.clone(),
            viewer,
            generation,
        })
    } else {
        None
    };
    let mut state = runtime.shared.subscribe_state();
    let mut notices = runtime.shared.subscribe_notices();
    let mut host_notices = runtime.host.notices();
    let mut frames = runtime.host.subscribe_screencast();
    let latest = runtime.host.latest_frame();
    Box::pin(async_stream::stream! {
        let _lease = lease;
        let current = state.borrow_and_update().clone();
        yield runtime.redact_event(SurfaceEvent::State(current));
        if let Some(frame) = latest {
            yield SurfaceEvent::Frame(frame);
        }
        if let Err(error) = runtime.ensure_page().await {
            yield runtime.redact_event(SurfaceEvent::Notice(SurfaceNotice::error(format!("the browser could not start: {error}"))));
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
                    Ok(notice) => Some(SurfaceEvent::Notice(host_notice(notice))),
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => None,
                },
            };
            match next {
                Some(event) => yield runtime.redact_event(event),
                None => break,
            }
        }
    })
}

struct ViewerLease {
    runtime: Arc<BrowserRuntime>,
    viewer: String,
    generation: u64,
}

impl Drop for ViewerLease {
    fn drop(&mut self) {
        let runtime = self.runtime.clone();
        let viewer = self.viewer.clone();
        let generation = self.generation;
        tokio::spawn(async move {
            let _guard = runtime.action_gate.lock().await;
            let mut model = runtime.viewport.lock().await;
            if model.leave(&viewer, generation) {
                log::debug!(
                    "[browser-viewport] leave viewer={viewer} lease={generation} owner={:?}",
                    model.owner()
                );
                if let Err(error) = runtime.apply_viewer_viewport(&mut model).await {
                    runtime
                        .shared
                        .notice(SurfaceNotice::error(format!("resize failed: {error}")));
                }
            }
        });
    }
}

#[cfg(test)]
mod tests;
