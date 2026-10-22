use super::*;

fn directory_link(target: &Path, link: &Path) {
    #[cfg(windows)]
    {
        let mut command = std::process::Command::new("cmd");
        crate::process_util::no_window_std(&mut command);
        let output = command
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[test]
fn safe_tree_removes_whole_lane_without_entering_node_modules_junction() {
    let fixture = tempfile::tempdir().unwrap();
    let outside = fixture.path().join("outside");
    let lane = fixture.path().join("lane");
    fs::create_dir_all(&outside).unwrap();
    fs::create_dir_all(lane.join("target/debug")).unwrap();
    fs::write(outside.join("proof"), "preserved").unwrap();
    fs::write(lane.join("target/debug/artifact"), "removed").unwrap();
    directory_link(&outside, &lane.join("node_modules"));
    remove(&lane).unwrap();
    assert!(!lane.exists());
    assert_eq!(
        fs::read_to_string(outside.join("proof")).unwrap(),
        "preserved"
    );
}

#[test]
fn safe_tree_refuses_junction_in_lane_ancestors() {
    let fixture = tempfile::tempdir().unwrap();
    let outside = fixture.path().join("outside");
    fs::create_dir_all(outside.join("lane")).unwrap();
    directory_link(&outside, &fixture.path().join("alias"));
    assert!(refuse_linked_ancestors(&fixture.path().join("alias/lane"))
        .unwrap_err()
        .contains("reparse point or symlink"));
    assert!(outside.join("lane").is_dir());
    #[cfg(windows)]
    fs::remove_dir(fixture.path().join("alias")).unwrap();
    #[cfg(unix)]
    fs::remove_file(fixture.path().join("alias")).unwrap();
}

#[test]
fn safe_tree_removes_root_through_symlinked_ancestor_without_entering_inner_links() {
    let fixture = tempfile::tempdir().unwrap();
    let actual = fixture.path().join("actual");
    let lane = actual.join("lane");
    let outside = fixture.path().join("outside");
    fs::create_dir_all(lane.join("target")).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("proof"), "preserved").unwrap();
    fs::write(lane.join("target/artifact"), "removed").unwrap();
    directory_link(&outside, &lane.join("node_modules"));
    let alias = fixture.path().join("alias");
    directory_link(&actual, &alias);
    remove(&alias.join("lane")).unwrap();
    assert!(!lane.exists());
    assert!(alias.is_dir());
    assert_eq!(
        fs::read_to_string(outside.join("proof")).unwrap(),
        "preserved"
    );
    #[cfg(windows)]
    fs::remove_dir(&alias).unwrap();
    #[cfg(unix)]
    fs::remove_file(&alias).unwrap();
}

#[test]
fn safe_tree_refuses_linked_root_without_removing_target() {
    let fixture = tempfile::tempdir().unwrap();
    let outside = fixture.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("proof"), "preserved").unwrap();
    let alias = fixture.path().join("alias");
    directory_link(&outside, &alias);
    assert!(remove(&alias)
        .unwrap_err()
        .contains("reparse point or symlink"));
    assert_eq!(
        fs::read_to_string(outside.join("proof")).unwrap(),
        "preserved"
    );
    #[cfg(windows)]
    fs::remove_dir(&alias).unwrap();
    #[cfg(unix)]
    fs::remove_file(&alias).unwrap();
}

#[cfg(windows)]
#[test]
fn safe_tree_removes_long_paths_and_readonly_files() {
    let fixture = tempfile::tempdir().unwrap();
    let lane = fixture.path().join("lane");
    let mut deep = extended_path(&lane).unwrap();
    for _ in 0..8 {
        deep.push("long-build-output-component-1234567890");
    }
    fs::create_dir_all(&deep).unwrap();
    let file = deep.join("artifact");
    fs::write(&file, "build output").unwrap();
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions).unwrap();
    remove(&lane).unwrap();
    assert!(!lane.exists());
}

#[cfg(windows)]
#[test]
fn safe_tree_failure_reports_exact_path_and_holder_evidence() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = tempfile::tempdir().unwrap();
    let lane = fixture.path().join("lane");
    fs::create_dir(&lane).unwrap();
    let file = lane.join("held-artifact");
    let handle = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .share_mode(0)
        .open(&file)
        .unwrap();
    let error = remove(&lane).unwrap_err();
    assert!(error.contains("held-artifact"), "{error}");
    assert!(error.contains("holder:"), "{error}");
    assert!(lane.exists());
    drop(handle);
    remove(&lane).unwrap();
}
