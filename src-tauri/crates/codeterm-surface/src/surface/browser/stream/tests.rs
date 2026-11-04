use super::*;
use crate::surface::runtime::NoticeKind;

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
