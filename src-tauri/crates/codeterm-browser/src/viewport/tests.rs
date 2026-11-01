use super::*;

#[test]
fn resize_model_coalesces_to_latest_only_after_acknowledgement() {
    let mut resize = ResizeModel::default();
    let first = Viewport::default();
    let middle = Viewport {
        width: 1000,
        ..first
    };
    let latest = Viewport {
        width: 800,
        ..first
    };
    assert_eq!(resize.begin(first), Some(first));
    assert_eq!(resize.begin(middle), None);
    assert_eq!(resize.begin(latest), None);
    resize.acknowledge(true);
    assert_eq!(resize.begin(latest), Some(latest));
    resize.acknowledge(true);
    assert_eq!(resize.begin(latest), None);
}

#[test]
fn resize_model_retries_an_unacknowledged_viewport_after_failure() {
    let mut resize = ResizeModel::default();
    let viewport = Viewport::default();
    assert_eq!(resize.begin(viewport), Some(viewport));
    resize.acknowledge(false);
    assert_eq!(resize.begin(viewport), Some(viewport));
    resize.acknowledge(true);
    assert_eq!(resize.begin(viewport), None);
}

#[test]
fn resize_model_drops_pending_changes_when_the_latest_returns_to_the_acknowledged_size() {
    let mut resize = ResizeModel::default();
    let initial = Viewport::default();
    let temporary = Viewport {
        width: 800,
        ..initial
    };
    assert_eq!(resize.begin(initial), Some(initial));
    assert_eq!(resize.begin(temporary), None);
    resize.acknowledge(true);
    assert_eq!(resize.begin(initial), None);
}

fn request(width: f64, height: f64, dpr: f64) -> ViewportRequest {
    ViewportRequest { width, height, dpr }
}

#[test]
fn browser_viewport_clamps_size_and_dpr() {
    assert_eq!(
        clamp(request(100.4, 50.0, 5.0)),
        Some(Viewport {
            width: MIN_WIDTH,
            height: MIN_HEIGHT,
            dpr: MAX_DPR,
            mobile: false,
        })
    );
    assert_eq!(
        clamp(request(1280.6, 720.2, 1.5)),
        Some(Viewport {
            width: 1281,
            height: 720,
            dpr: 1.5,
            mobile: false,
        })
    );
    assert_eq!(
        clamp(request(99999.0, 99999.0, 0.5)).unwrap().width,
        MAX_WIDTH
    );
    assert_eq!(clamp(request(800.0, 600.0, 0.5)).unwrap().dpr, MIN_DPR);
    assert_eq!(clamp(request(800.0, 600.0, f64::NAN)).unwrap().dpr, MIN_DPR);
}

#[test]
fn browser_viewport_rejects_non_positive_or_non_finite_size() {
    assert_eq!(clamp(request(0.0, 600.0, 1.0)), None);
    assert_eq!(clamp(request(800.0, -1.0, 1.0)), None);
    assert_eq!(clamp(request(f64::INFINITY, 600.0, 1.0)), None);
    assert_eq!(clamp(request(f64::NAN, 600.0, 1.0)), None);
}

#[test]
fn browser_viewport_device_size_scales_by_dpr() {
    let viewport = Viewport {
        width: 1280,
        height: 721,
        dpr: 1.5,
        mobile: false,
    };
    assert_eq!(viewport.device_size(), (1920, 1082));
}

#[test]
fn browser_viewport_owned_mode_resizes_once_per_change() {
    let mut model = ViewportModel::new(ViewportMode::Owned, Viewport::default());
    let first = model.apply(request(1280.0, 720.0, 2.0));
    assert!(matches!(first, ViewportDecision::Resize(v) if v.width == 1280 && v.dpr == 2.0));
    assert_eq!(
        model.apply(request(1280.0, 720.0, 2.0)),
        ViewportDecision::Unchanged
    );
    assert_eq!(
        model.apply(request(0.0, 0.0, 1.0)),
        ViewportDecision::Invalid
    );
    assert_eq!(model.current().width, 1280);
}

#[test]
fn browser_viewport_attach_mode_never_resizes() {
    let initial = Viewport::default();
    let mut model = ViewportModel::new(ViewportMode::Attached, initial);
    assert_eq!(
        model.apply(request(800.0, 600.0, 1.0)),
        ViewportDecision::RefusedAttached
    );
    assert_eq!(model.current(), initial);
}

#[test]
fn browser_mobile_viewport_preserved_by_viewer_resize_and_refused_on_attach() {
    let mut model = ViewportModel::new(ViewportMode::Owned, Viewport::default());
    model.apply_device(request(390.0, 844.0, 2.0), true);
    model.apply(request(400.0, 800.0, 2.0));
    assert!(model.current().mobile);
    let mut attached = ViewportModel::new(ViewportMode::Attached, Viewport::default());
    assert_eq!(
        attached.apply_device(request(390.0, 844.0, 2.0), true),
        ViewportDecision::RefusedAttached
    );
    assert!(!attached.current().mobile);
}
