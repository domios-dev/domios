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

#[test]
fn expands_versioned_desktop_cli_wildcards_newest_first_and_rejects_unsafe_globs() {
    let home = PathBuf::from(if cfg!(windows) {
        r"C:\Users\fixture"
    } else {
        "/home/fixture"
    });
    let appdata = home.join("AppData").join("Roaming");
    let env = |key: &str| match key {
        "APPDATA" => Some(appdata.clone().into_os_string()),
        _ => None,
    };

    let claude_win_parent = appdata.join("Claude").join("claude-code");
    let claude_mac_parent = home
        .join("Library")
        .join("Application Support")
        .join("Claude")
        .join("claude-code");
    let opencode_win_parent = appdata.join("ai.opencode.desktop").join("cli");

    let reader = |dir: &Path| {
        if dir == claude_win_parent {
            vec![
                "2.1.9".into(),
                "2.1.87".into(),
                "2.1.10".into(),
                "..".into(),
                "bad/child".into(),
            ]
        } else if dir == claude_mac_parent {
            vec!["2.1.50".into(), "2.1.87".into()]
        } else if dir == opencode_win_parent {
            vec!["1.19.6".into(), "2.4.2-x64".into(), "custom".into()]
        } else {
            Vec::new()
        }
    };

    let templates = vec![
        "%APPDATA%/Claude/claude-code/*".into(),
        "~/Library/Application Support/Claude/claude-code/*/claude.app/Contents/MacOS".into(),
        "%APPDATA%/ai.opencode.desktop/cli/*".into(),
        "%APPDATA%/Claude/claude-code/*/*".into(),
        "%APPDATA%/Claude/claude-code/2.*".into(),
        "%APPDATA%/Claude/claude-code/*/../escape".into(),
        "/*".into(),
    ];

    let expanded = expand_install_dirs_with_reader(&templates, &env, Some(&home), &reader);
    assert_eq!(
        expanded,
        vec![
            claude_win_parent.join("2.1.87"),
            claude_win_parent.join("2.1.10"),
            claude_win_parent.join("2.1.9"),
            claude_mac_parent
                .join("2.1.87")
                .join("claude.app")
                .join("Contents")
                .join("MacOS"),
            claude_mac_parent
                .join("2.1.50")
                .join("claude.app")
                .join("Contents")
                .join("MacOS"),
            opencode_win_parent.join("2.4.2-x64"),
            opencode_win_parent.join("1.19.6"),
            opencode_win_parent.join("custom"),
        ]
    );
}
