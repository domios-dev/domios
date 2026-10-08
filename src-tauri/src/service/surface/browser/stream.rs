use std::sync::Arc;

use tokio::sync::broadcast::error::RecvError;

use super::BrowserRuntime;
use crate::service::browser::screencast::{HubNotice, HubNoticeKind};
use crate::service::browser::viewport::ViewportRequest;
use crate::service::surface::runtime::{SurfaceEvent, SurfaceEvents, SurfaceNotice};

fn host_notice(notice: HubNotice) -> SurfaceNotice {
    match notice.kind {
        HubNoticeKind::Plain => SurfaceNotice::plain(notice.text),
        HubNoticeKind::PreviewPaused => SurfaceNotice::preview_paused(notice.text),
    }
}

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
                    Ok(notice) => Some(SurfaceEvent::Notice(host_notice(notice))),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::surface::runtime::NoticeKind;

    #[test]
    fn browser_stream_maps_still_failures_to_preview_paused_and_keeps_other_host_notices() {
        let paused = host_notice(HubNotice {
            kind: HubNoticeKind::PreviewPaused,
            text: "no frame: the still capture returned no image".into(),
        });
        assert_eq!(paused.kind, Some(NoticeKind::PreviewPaused));
        assert_eq!(
            paused.detail.as_deref(),
            Some("no frame: the still capture returned no image")
        );
        assert_eq!(paused.tone, None);
        let plain = host_notice(HubNotice {
            kind: HubNoticeKind::Plain,
            text: "screencast could not start: x".into(),
        });
        assert_eq!(plain, SurfaceNotice::plain("screencast could not start: x"));
    }
}
