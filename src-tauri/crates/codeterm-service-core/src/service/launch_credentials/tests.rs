use super::*;
use crate::service::agent_pad::{AgentPad, AgentPadRole};

fn directory_link(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        let mut command = std::process::Command::new("cmd");
        codeterm_util::process_util::no_window_std(&mut command);
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
}

fn unlink_directory_link(path: &Path) {
    #[cfg(unix)]
    fs::remove_file(path).unwrap();
    #[cfg(windows)]
    fs::remove_dir(path).unwrap();
}

#[test]
fn reset_removes_stale_credentials_and_preserves_agent_pads() {
    let root = tempfile::tempdir().unwrap();
    let pad = AgentPad::create(root.path(), "retained", AgentPadRole::Worker).unwrap();
    fs::write(pad.path().join("notes.md"), b"keep").unwrap();
    let directory = Directory::reset(root.path()).unwrap();
    assert!(codeterm_util::private_file::open_directory_owner_only(&directory.path).is_ok());
    for name in [
        ".runtime-launch-old.json",
        ".launch-credential-old.txt",
        "old.tmp",
    ] {
        fs::write(directory.path.join(name), b"stale").unwrap();
    }
    let restored = Directory::reset(root.path()).unwrap();
    assert_eq!(restored.path, directory.path);
    assert_eq!(fs::read_dir(&restored.path).unwrap().count(), 0);
    assert!(codeterm_util::private_file::open_directory_owner_only(&restored.path).is_ok());
    assert_eq!(fs::read(pad.path().join("notes.md")).unwrap(), b"keep");
    assert!(pad.path().join("report.md").is_file());
}

#[test]
fn reset_refuses_a_replacement_directory_link() {
    let root = tempfile::tempdir().unwrap();
    let directory = Directory::reset(root.path()).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let victim = outside.path().join(".runtime-launch-victim.json");
    fs::write(&victim, b"outside").unwrap();
    fs::rename(&directory.path, root.path().join("retired")).unwrap();
    directory_link(outside.path(), &directory.path);
    let error = directory.clear_and_create().unwrap_err();
    assert!(error.contains("ordinary directory"));
    assert_eq!(fs::read(victim).unwrap(), b"outside");
    assert!(codeterm_util::safe_tree::is_link(
        &fs::symlink_metadata(directory.path).unwrap()
    ));
}

#[test]
fn reset_refuses_directory_link_entries_without_touching_their_targets() {
    let root = tempfile::tempdir().unwrap();
    let directory = Directory::reset(root.path()).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let victim = outside.path().join(".launch-credential-victim.txt");
    fs::write(&victim, b"outside").unwrap();
    directory_link(outside.path(), &directory.path.join("replacement"));
    let error = directory.clear_and_create().unwrap_err();
    assert!(error.contains("non-regular launch credential entry"));
    assert_eq!(fs::read(victim).unwrap(), b"outside");
    assert!(directory.path.join("replacement").exists());
}

#[test]
fn reset_refuses_unexpected_child_directories_without_descending() {
    let root = tempfile::tempdir().unwrap();
    let directory = Directory::reset(root.path()).unwrap();
    let nested = directory.path.join("nested");
    fs::create_dir(&nested).unwrap();
    let retained = nested.join(".runtime-launch-keep.json");
    fs::write(&retained, b"keep").unwrap();
    let error = directory.clear_and_create().unwrap_err();
    assert!(error.contains("non-regular launch credential entry"));
    assert_eq!(fs::read(retained).unwrap(), b"keep");
}

#[cfg(unix)]
#[test]
fn reset_refuses_file_symlinks_without_touching_their_targets() {
    let root = tempfile::tempdir().unwrap();
    let directory = Directory::reset(root.path()).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let victim = outside.path().join(".launch-credential-victim.txt");
    fs::write(&victim, b"outside").unwrap();
    let link = directory.path.join("file-link");
    std::os::unix::fs::symlink(&victim, &link).unwrap();
    let error = directory.clear_and_create().unwrap_err();
    assert!(error.contains("non-regular launch credential entry"));
    assert_eq!(fs::read(victim).unwrap(), b"outside");
    assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
}

#[test]
fn linked_runtime_ancestor_is_resolved_once_before_cleanup_and_launches() {
    let root = tempfile::tempdir().unwrap();
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    directory_link(first.path(), &root.path().join("runtime"));
    let directory = Directory::resolve(root.path()).unwrap();
    assert_eq!(
        directory.path.parent().unwrap(),
        fs::canonicalize(first.path()).unwrap()
    );
    fs::create_dir(second.path().join("launch-credentials")).unwrap();
    let victim = second
        .path()
        .join("launch-credentials/.runtime-launch-victim.json");
    fs::write(&victim, b"outside").unwrap();
    unlink_directory_link(&root.path().join("runtime"));
    directory_link(second.path(), &root.path().join("runtime"));
    directory.clear_and_create().unwrap();
    let path = directory.file_path(codeterm_util::tab_launch::RUNTIME_FILE_PREFIX, ".json");
    let credential = codeterm_util::tab_launch::LaunchCredential {
        tab_id: "own-tab".into(),
        binding_nonce: "own-nonce".into(),
        token: "own-token".into(),
    };
    codeterm_util::tab_launch::write_new(&path, &serde_json::to_vec(&credential).unwrap()).unwrap();
    assert_eq!(
        codeterm_util::tab_launch::resolve(None, Some(&path), Some("own-tab"), Some("own-nonce"))
            .unwrap(),
        "own-token"
    );
    assert_eq!(fs::read(victim).unwrap(), b"outside");
    assert!(path.starts_with(fs::canonicalize(first.path()).unwrap()));
}
