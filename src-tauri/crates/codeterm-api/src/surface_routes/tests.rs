use super::*;

#[test]
fn surface_stream_viewport_needs_width_and_height() {
    let query = StreamQuery {
        w: Some(1280.0),
        h: Some(720.0),
        dpr: None,
    };
    assert_eq!(
        stream_viewport(&query),
        Some(ViewportRequest {
            width: 1280.0,
            height: 720.0,
            dpr: 1.0
        })
    );
    assert_eq!(stream_viewport(&StreamQuery::default()), None);
}

#[test]
fn surface_stream_refusals_are_conflicts_for_non_stream_tabs() {
    let (status, body) = surface_refusal(SurfaceError::NotAStreamSurface("t".into()));
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body.0["error"]["code"], "not_a_stream_surface");
    let (status, _) = surface_refusal(SurfaceError::TabNotFound("t".into()));
    assert_eq!(status, StatusCode::NOT_FOUND);
}
