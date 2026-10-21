use super::TrailModel;

#[test]
fn launch_pause_replays_trail_lines_in_order_after_resume() {
    let mut trail = TrailModel::default();
    assert_eq!(
        trail.record("before launch".into()),
        Some(vec!["before launch".to_string()])
    );

    trail.pause();
    assert_eq!(trail.record("launch started".into()), Some(Vec::new()));
    assert_eq!(trail.record("server ready".into()), Some(Vec::new()));

    assert_eq!(
        trail.resume(),
        vec!["launch started".to_string(), "server ready".to_string()]
    );
    assert_eq!(
        trail.record("after resume".into()),
        Some(vec!["after resume".to_string()])
    );
}
