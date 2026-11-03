use super::*;
#[test]
fn browser_region_scale_capture_validates_bounds_and_combinations() {
    let options = parse(
        &args(&["zoom.png", "--region", "10,20,300,400", "--scale", "2"]),
        false,
    )
    .unwrap();
    assert_eq!(options.region, Some([10.0, 20.0, 300.0, 400.0]));
    assert_eq!(options.scale, Some(2.0));
    for words in [
        vec!["--region"],
        vec!["--region", "0,0,1"],
        vec!["--region", "0,0,0,1"],
        vec!["--region", "-1,0,1,1"],
        vec!["--region", "NaN,0,1,1"],
        vec!["--region", "0,0,10000,10000"],
        vec!["--region", "0,0,1,1", "--full"],
        vec!["--scale"],
        vec!["--scale", "0"],
        vec!["--scale", "NaN"],
        vec!["--scale", "5"],
        vec!["--scale", "2", "--scale", "3"],
        vec!["--viewport", "400x500", "--scale", "2"],
    ] {
        assert!(parse(&args(&words), false).is_err(), "{words:?}");
    }
    assert!(parse(&args(&["set", "400", "500", "--scale", "2"]), true).is_err());
}
fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|s| s.to_string()).collect()
}
#[test]
fn browser_mobile_metrics_options_preserve_capture_name() {
    let out = parse(
        &args(&[
            "phone.png",
            "--viewport",
            "390x844",
            "--mobile",
            "--dpr",
            "2",
            "--restore",
            "--metrics-json",
        ]),
        false,
    )
    .unwrap();
    assert_eq!(out.name.as_deref(), Some("phone.png"));
    assert!(out.restore && out.metrics);
    assert_eq!(
        out.viewport,
        Some(Viewport {
            width: 390,
            height: 844,
            dpr: 2.0,
            mobile: true
        })
    );
}
#[test]
fn browser_mobile_metrics_reject_invalid_options() {
    for words in [
        vec!["--viewport", "x"],
        vec!["--mobile"],
        vec!["--restore"],
        vec!["--viewport", "390x844", "--dpr", "NaN"],
    ] {
        assert!(parse(&args(&words), false).is_err());
    }
}
