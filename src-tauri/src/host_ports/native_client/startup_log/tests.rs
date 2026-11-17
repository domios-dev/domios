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

#[test]
fn a_new_client_preserves_the_previous_session_and_truncates_only_current() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("client-startup.log");
    let previous = directory.path().join("client-startup.prev.log");
    std::fs::write(&path, "failed session\n").unwrap();
    std::fs::write(&previous, "older session\n").unwrap();
    drop(super::open_session_at(&path).unwrap());
    assert_eq!(
        std::fs::read_to_string(&previous).unwrap(),
        "failed session\n"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
}

#[test]
fn failure_records_are_written_once_with_a_two_hundred_record_bound() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("client-startup.log");
    let mut trail = super::Trail {
        file: super::open_session_at(&path),
        model: TrailModel::default(),
    };
    for index in 0..super::MAX_LINES + 1 {
        let line = format!(
            "frontend unavailable: code=startup_stream_failed reason=readiness failed {index}"
        );
        if let Some(lines) = trail.model.record(line) {
            super::write_lines(&mut trail, Some(&path), lines);
        }
    }
    let text = std::fs::read_to_string(path).unwrap();
    assert_eq!(text.lines().count(), 200);
    assert!(text.lines().next().unwrap().ends_with("readiness failed 0"));
    assert!(text
        .lines()
        .last()
        .unwrap()
        .ends_with("readiness failed 199"));
}
