use super::*;

#[test]
fn log_rotation_caps_generations_and_closes_windows_handles_before_rename() {
    let root = tempfile::tempdir().unwrap();
    let mut writer = RotatingLog::open(root.path().into()).unwrap();
    let chunk = vec![b'x'; MAX_BYTES as usize];
    for _ in 0..5 {
        writer.write_all(&chunk).unwrap();
    }
    writer.flush().unwrap();
    let entries: Vec<_> = std::fs::read_dir(root.path()).unwrap().collect();
    assert_eq!(entries.len(), GENERATIONS + 1);
    for entry in entries {
        assert!(entry.unwrap().metadata().unwrap().len() <= MAX_BYTES);
    }
}

#[test]
fn client_logger_accepts_only_structured_updater_events_at_release_levels() {
    let sink = NonBlockingLog::spawn("fixture", Vec::<u8>::new(), 4, Vec::new()).unwrap();
    let logger = ClientLogger(sink);
    use log::Log;
    for (target, level, expected) in [
        ("native_updater", log::Level::Info, true),
        ("native_updater", log::Level::Debug, false),
        ("reqwest", log::Level::Warn, false),
    ] {
        assert_eq!(
            logger.enabled(&log::Metadata::builder().target(target).level(level).build()),
            expected
        );
    }
}
