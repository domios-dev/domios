use super::*;
use std::fs;

#[test]
fn settings_files_release_only_superseded_owned_paths_then_close() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("runtime/task-files");
    let marker = instance_marker_for(temp.path());
    let old = write_at(&root, &marker, "tab", "old", Some(".ct-settings.json")).unwrap();
    let current = write_at(&root, &marker, "tab", "new", Some(".ct-settings.json")).unwrap();
    let sibling = write_at(&root, &marker, "other", "other", Some(".ct-settings.json")).unwrap();
    let foreign = temp.path().join("foreign.ct-settings.json");
    fs::write(&foreign, "user").unwrap();
    let owned = owned_paths(&root, "tab");
    assert!(owned.contains(&old) && owned.contains(&current));
    assert_eq!(
        release_paths(
            &root,
            "tab",
            &[old.clone(), sibling.clone(), foreign.clone()]
        ),
        vec![old.clone()]
    );
    assert!(!old.exists());
    assert!(current.exists() && sibling.exists() && foreign.exists());
    assert_eq!(release_tab(&root, "tab", &[]), vec![current.clone()]);
    assert!(!current.exists());
    assert!(sibling.exists() && foreign.exists());
}

#[test]
fn captured_files_land_in_the_instance_task_directory_and_are_owned_by_tab() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("runtime").join("task-files");
    let marker = instance_marker_for(temp.path());
    let (path, paths) = capture("tab-A", || {
        write_at(&root, &marker, "tab-A", "payload", Some(".ct-task.md")).unwrap()
    });
    assert_eq!(path.parent(), Some(root.as_path()));
    assert_eq!(paths, vec![path.clone()]);
    assert!(path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .contains(&encode(b"tab-A")));
    assert_eq!(fs::read_to_string(path).unwrap(), "payload");
}

#[test]
fn task_file_sweep_is_instance_scoped() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("runtime").join("task-files");
    fs::create_dir_all(&root).unwrap();
    let marker = instance_marker_for(temp.path());
    let live = root.join(format!(
        "{}live-abc.ct-task",
        file_prefix(&marker, "tab-live")
    ));
    let closed = root.join(format!(
        "{}closed-def.ct-task",
        file_prefix(&marker, "tab-closed")
    ));
    let foreign = root.join("codeterm-plugin-another-instance-66-foreign.ct-task");
    fs::write(&live, "live").unwrap();
    fs::write(&closed, "closed").unwrap();
    fs::write(&foreign, "foreign").unwrap();

    assert_eq!(
        release_tab(&root, "tab-closed", &[closed.clone()]),
        vec![closed.clone()]
    );
    assert!(sweep(&root, &HashSet::from(["tab-live".to_string()])).is_empty());
    assert!(live.exists());
    assert!(foreign.exists());
    let orphan = root.join(format!(
        "{}orphan-ghi.ct-task",
        file_prefix(&marker, "tab-orphan")
    ));
    fs::write(&orphan, "orphan").unwrap();
    assert_eq!(
        sweep(&root, &HashSet::from(["tab-live".to_string()])),
        vec![orphan.clone()]
    );
    assert!(foreign.exists());
}
