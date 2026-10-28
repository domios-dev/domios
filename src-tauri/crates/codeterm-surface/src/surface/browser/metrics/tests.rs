use super::*;
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
