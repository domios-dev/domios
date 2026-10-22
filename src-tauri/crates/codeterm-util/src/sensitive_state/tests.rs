use super::*;

#[test]
fn only_sensitive_files_are_selected_and_unrelated_trees_are_untouched() {
    let root = tempfile::tempdir().unwrap();
    for name in [
        "bin/tool",
        "runtime/agent-pads/pad.json",
        "browsers/deep/profile.json",
        "state.json",
        "mesh/identity.json",
        "plugins/example/config.yaml",
        "plugins/example/plugin.js",
    ] {
        let path = root.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"value").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        }
    }
    tighten_existing(root.path());
    for name in [
        "state.json",
        "mesh/identity.json",
        "plugins/example/config.yaml",
    ] {
        assert!(crate::private_file::owner_only(&root.path().join(name)).unwrap());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in [
            "bin/tool",
            "runtime/agent-pads/pad.json",
            "browsers/deep/profile.json",
            "plugins/example/plugin.js",
        ] {
            assert_eq!(
                fs::metadata(root.path().join(name))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o644
            );
        }
    }
}

#[test]
fn missing_and_non_regular_entries_do_not_abort_other_sensitive_files() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".secrets")).unwrap();
    fs::write(root.path().join("state.json"), b"state").unwrap();
    tighten_existing(root.path());
    assert!(crate::private_file::owner_only(&root.path().join("state.json")).unwrap());
    tighten_existing(&root.path().join("vanished"));
}
