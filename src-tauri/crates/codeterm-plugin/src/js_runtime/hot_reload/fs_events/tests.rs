use super::*;
use std::time::Duration;

#[test]
fn plugin_watch_roots_partition_ignored_top_level_directories() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::write(root.join(".gitignore"), "/output/\n").unwrap();
    std::fs::create_dir(root.join("output")).unwrap();
    std::fs::create_dir(root.join("source")).unwrap();
    let watched = desired_watches(&[root.to_path_buf()]).unwrap();
    assert_eq!(watched.get(root), Some(&RecursiveMode::NonRecursive));
    assert_eq!(
        watched.get(&root.join("source")),
        Some(&RecursiveMode::Recursive)
    );
    assert!(!watched.contains_key(&root.join("output")));
}

#[test]
fn plugin_reads_and_unrelated_files_do_not_schedule_reloads() {
    let roots = vec![PathBuf::from("/plugins")];
    let read = Event::new(EventKind::Access(AccessKind::Read))
        .add_path("/plugins/provider/plugin.js".into());
    let unrelated = Event::new(EventKind::Any).add_path("/sessions/output.log".into());
    let changed = Event::new(EventKind::Any).add_path("/plugins/provider/plugin.js".into());
    assert!(!relevant(&read, &roots));
    assert!(!relevant(&unrelated, &roots));
    assert!(relevant(&changed, &roots));
}

#[test]
fn filesystem_changes_wake_the_reconciler_without_a_polling_tick() {
    let directory = tempfile::tempdir().unwrap();
    let events = FileEvents::new(vec![directory.path().to_path_buf()]).unwrap();
    let started = std::time::Instant::now();
    std::fs::write(directory.path().join("plugin.json"), "{}").unwrap();
    events
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    eprintln!("plugin filesystem wake: {:?}", started.elapsed());
}

#[test]
fn a_missing_plugin_root_is_observed_through_its_existing_parent() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("plugins");
    let mut events = FileEvents::new(vec![root.clone()]).unwrap();
    std::fs::create_dir(&root).unwrap();
    events
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    events.refresh().unwrap();
    assert_eq!(events.watched.get(&root), Some(&RecursiveMode::Recursive));
    std::fs::remove_dir(&root).unwrap();
    events
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    events.refresh().unwrap();
    assert!(!events.watched.contains_key(&root));
}
