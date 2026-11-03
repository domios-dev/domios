use super::*;

#[test]
fn owned_capture_uses_css_bounds_and_unit_scale_at_every_device_density() {
    for mobile in [false, true] {
        for dpr in [1.0, 1.25, 1.5, 2.0] {
            let metrics = json!({
                "devicePixelRatio":dpr,"mobile":mobile,
                "cssLayoutViewport":{"pageX":10,"pageY":222,"clientWidth":380,"clientHeight":844},
                "cssContentSize":{"x":0,"y":0,"width":400,"height":2000},
                "layoutViewport":{"clientWidth":380.0*dpr,"clientHeight":844.0*dpr}
            });
            let viewport = screenshot_params(
                json!({"format":"png","captureBeyondViewport":false}),
                &metrics,
            )
            .unwrap();
            assert_eq!(
                viewport,
                json!({"format":"png","captureBeyondViewport":false,
                "clip":{"x":10.0,"y":222.0,"width":380.0,"height":844.0,"scale":1}})
            );
            let full = screenshot_params(
                json!({"format":"png","captureBeyondViewport":true}),
                &metrics,
            )
            .unwrap();
            assert_eq!(
                full,
                json!({"format":"png","captureBeyondViewport":true,
                "clip":{"x":0.0,"y":0.0,"width":400.0,"height":2000.0,"scale":1}})
            );
        }
    }
}

#[test]
fn explicit_region_and_scale_survive_owned_capture_parameter_resolution() {
    let params = json!({"format":"jpeg","quality":90,"captureBeyondViewport":true,
        "clip":{"x":20,"y":40,"width":100,"height":200,"scale":2}});
    assert_eq!(
        screenshot_params(params.clone(), &json!({})).unwrap(),
        params
    );
    assert!(screenshot_params(json!({"format":"png"}), &json!({})).is_err());
    assert!(screenshot_params(
        json!({"format":"png"}),
        &json!({"cssLayoutViewport":{"clientWidth":380,"clientHeight":0}})
    )
    .is_err());
}
