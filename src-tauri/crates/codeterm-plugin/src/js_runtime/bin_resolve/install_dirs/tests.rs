use super::*;

#[test]
fn expands_home_env_and_absolute_install_dirs_and_rejects_traversal() {
    let home = PathBuf::from(if cfg!(windows) {
        r"C:\Users\fixture"
    } else {
        "/home/fixture"
    });
    let env = |key: &str| match key {
        "FIXTURE_INSTALL_DIR" => Some(OsString::from(if cfg!(windows) {
            r"C:\opt\fixture-tool"
        } else {
            "/opt/fixture-tool"
        })),
        "APPDATA" => Some(home.join("Roaming").into_os_string()),
        "EMPTY_VAR" => Some(OsString::new()),
        _ => None,
    };

    let templates = vec![
        "$FIXTURE_INSTALL_DIR".into(),
        "${FIXTURE_INSTALL_DIR}/bin".into(),
        "~/.fixture-tool/bin".into(),
        "%APPDATA%/npm".into(),
        "$EMPTY_VAR/bin".into(),
        "$MISSING_VAR/bin".into(),
        "~/../escape".into(),
        "%APPDATA%/../escape".into(),
        "relative/path".into(),
        "~/.fixture-tool/bin".into(),
    ];

    let expanded = expand_install_dirs(&templates, &env, Some(&home));
    assert_eq!(
        expanded,
        vec![
            PathBuf::from(if cfg!(windows) {
                r"C:\opt\fixture-tool"
            } else {
                "/opt/fixture-tool"
            }),
            PathBuf::from(if cfg!(windows) {
                r"C:\opt\fixture-tool"
            } else {
                "/opt/fixture-tool"
            })
            .join("bin"),
            home.join(".fixture-tool").join("bin"),
            home.join("Roaming").join("npm"),
        ]
    );
}

#[test]
fn npm_global_bin_dirs_cover_env_npmrc_and_windows_appdata() {
    let root = std::env::temp_dir().join(format!(
        "domios_npmrc_fixture_{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join(".npmrc"),
        "# comment\n; another\nprefix=\"~/.custom-npm\"\n",
    )
    .unwrap();
    let env = |key: &str| match key {
        "NPM_CONFIG_PREFIX" => Some(root.join("env-prefix").into_os_string()),
        "APPDATA" => Some(root.join("Roaming").into_os_string()),
        _ => None,
    };

    let unix_dirs = npm_global_bin_dirs(&env, Some(&root), false);
    assert_eq!(
        unix_dirs,
        vec![
            root.join("env-prefix").join("bin"),
            root.join(".custom-npm").join("bin"),
        ]
    );

    let win_dirs = npm_global_bin_dirs(&env, Some(&root), true);
    assert_eq!(
        win_dirs,
        vec![
            root.join("env-prefix"),
            root.join(".custom-npm"),
            root.join("Roaming").join("npm"),
        ]
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn npm_and_manifest_roots_require_absolute_paths_without_traversal() {
    let home = tempfile::tempdir().unwrap();
    let unsafe_root = home.path().join("..").join("elsewhere");
    for invalid in [Path::new("relative"), unsafe_root.as_path()] {
        let env = |_: &str| Some(invalid.as_os_str().to_owned());
        assert!(npm_global_bin_dirs(&env, Some(home.path()), true).is_empty());
        assert!(expand_install_dir("${NPM_CONFIG_PREFIX}/bin", &env, Some(home.path())).is_none());
        assert_eq!(
            parse_npmrc_prefix(&format!("prefix={}", invalid.display()), Some(home.path())),
            None
        );
    }
}

#[test]
fn npmrc_prefix_refreshes_when_mtime_changes_without_length_change() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join(".npmrc");
    for (value, seconds) in [("one", 1_000_000), ("two", 2_000_000)] {
        std::fs::write(&path, format!("prefix=~/{value}")).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(seconds))
            .unwrap();
        assert_eq!(
            read_npmrc_prefix(&path, Some(home.path())),
            Some(home.path().join(value))
        );
    }
}
