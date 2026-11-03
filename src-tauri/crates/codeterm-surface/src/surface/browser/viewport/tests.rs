use super::*;

fn size(width: f64, height: f64) -> ViewportRequest {
    ViewportRequest {
        width,
        height,
        dpr: 1.0,
    }
}

fn join(model: &mut ViewportOwner, viewer: String, viewport: ViewportRequest) -> Option<u64> {
    model.join(viewer, viewport, false)
}

#[test]
fn two_viewers_cannot_ping_pong_the_shared_viewport() {
    let mut model = ViewportOwner::default();
    let outer = size(1034.0, 1150.0);
    let inner = size(474.0, 240.0);
    join(&mut model, "outer".into(), outer);
    assert_eq!(model.next_resize(), clamp(outer));
    join(&mut model, "inner".into(), inner);
    assert_eq!(model.owner().as_deref(), Some("outer"));
    assert_eq!(model.next_resize(), None);
    for _ in 0..20 {
        assert_eq!(model.request("inner", inner, false), None);
        model.request("outer", outer, false);
        assert_eq!(model.next_resize(), None);
    }
    assert!(model.request("inner", inner, true).is_some());
    assert_eq!(model.owner().as_deref(), Some("inner"));
    assert_eq!(model.next_resize(), clamp(inner));
    assert_eq!(model.request("outer", outer, false), None);
    assert_eq!(model.next_resize(), None);
}

#[test]
fn recursive_viewers_joining_after_a_resize_never_emit_another_resize() {
    let mut model = ViewportOwner::default();
    let outer = size(1034.0, 1150.0);
    join(&mut model, "outer".into(), outer);
    assert_eq!(model.next_resize(), clamp(outer));
    for depth in 1..32 {
        let viewer = format!("recursive-{depth}");
        let inner = size(1034.0 - f64::from(depth) * 20.0, 240.0);
        assert_eq!(model.join(viewer.clone(), inner, true), None);
        assert_eq!(model.request(&viewer, inner, false), None);
        assert_eq!(model.next_resize(), None);
        assert_eq!(model.owner().as_deref(), Some("outer"));
    }
}

#[test]
fn owner_departure_transfers_once_and_stale_disconnect_cannot_remove_reconnect() {
    let mut model = ViewportOwner::default();
    let old = join(&mut model, "first".into(), size(800.0, 600.0)).unwrap();
    model.next_resize();
    let second = join(&mut model, "second".into(), size(640.0, 480.0)).unwrap();
    let fresh = join(&mut model, "first".into(), size(800.0, 600.0)).unwrap();
    assert!(!model.leave("first", old));
    assert_eq!(model.owner().as_deref(), Some("first"));
    assert!(model.leave("first", fresh));
    assert_eq!(model.owner().as_deref(), Some("second"));
    assert_eq!(model.next_resize(), clamp(size(640.0, 480.0)));
    assert!(!model.leave("first", fresh));
    assert_eq!(model.next_resize(), None);
    assert!(model.leave("second", second));
    assert_eq!(model.owner(), None);
    assert_eq!(model.next_resize(), None);
}

#[test]
fn explicit_agent_viewport_survives_viewers_until_user_claims_control() {
    let mut model = ViewportOwner::default();
    let first = join(&mut model, "first".into(), size(800.0, 600.0)).unwrap();
    model.next_resize();
    model.agent();
    join(&mut model, "second".into(), size(640.0, 480.0));
    assert_eq!(model.request("first", size(900.0, 700.0), false), None);
    model.leave("first", first);
    assert_eq!(model.next_resize(), None);
    assert_eq!(model.owner(), None);
    assert!(model.request("second", size(640.0, 480.0), true).is_some());
    assert_eq!(model.next_resize(), clamp(size(640.0, 480.0)));
}

#[test]
fn invalid_or_unknown_requests_never_claim_and_identical_requests_emit_once() {
    let mut model = ViewportOwner::default();
    let viewport = size(800.0, 600.0);
    assert_eq!(
        join(&mut model, "invalid".into(), size(f64::NAN, 10.0)),
        None
    );
    assert_eq!(model.request("unknown", viewport, true), None);
    join(&mut model, "owner".into(), viewport);
    assert_eq!(model.next_resize(), clamp(viewport));
    assert_eq!(model.request("owner", size(0.0, 600.0), true), None);
    for _ in 0..10 {
        model.request("owner", viewport, true);
        assert_eq!(model.next_resize(), None);
    }
}

#[test]
fn a_recursive_viewer_never_inherits_or_claims_the_page_viewport() {
    assert!(recursive_viewer(Some("page-1"), Some("page-1")));
    assert!(!recursive_viewer(Some("page-1"), Some("page-2")));
    assert!(!recursive_viewer(None, None));
    assert!(!recursive_viewer(Some("page-1"), None));
    let mut model = ViewportOwner::default();
    let outer = model
        .join("outer".into(), size(1034.0, 1150.0), false)
        .unwrap();
    assert!(model.next_resize().is_some());
    assert_eq!(
        model.join("recursive".into(), size(474.0, 240.0), true),
        None
    );
    assert_eq!(model.request("recursive", size(474.0, 240.0), true), None);
    assert!(model.leave("outer", outer));
    assert_eq!(model.owner(), None);
    assert_eq!(model.next_resize(), None);
    assert_eq!(
        model.join("recursive-reconnect".into(), size(474.0, 240.0), true),
        None
    );
    assert_eq!(model.owner(), None);
    assert_eq!(model.next_resize(), None);
}
