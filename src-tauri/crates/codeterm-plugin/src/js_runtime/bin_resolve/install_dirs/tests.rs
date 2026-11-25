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
        "APPDATA" => Some(OsString::from(r"C:\Users\fixture\AppData\Roaming")),
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
            PathBuf::from(r"C:\Users\fixture\AppData\Roaming").join("npm"),
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
        "NPM_CONFIG_PREFIX" => Some(OsString::from("/env/prefix")),
        "APPDATA" => Some(OsString::from(r"D:\Roaming")),
        _ => None,
    };

    let unix_dirs = npm_global_bin_dirs(&env, Some(&root), false);
    assert_eq!(
        unix_dirs,
        vec![
            PathBuf::from("/env/prefix").join("bin"),
            root.join(".custom-npm").join("bin"),
        ]
    );

    let win_dirs = npm_global_bin_dirs(&env, Some(&root), true);
    assert_eq!(
        win_dirs,
        vec![
            PathBuf::from("/env/prefix"),
            root.join(".custom-npm"),
            PathBuf::from(r"D:\Roaming").join("npm"),
        ]
    );

    let _ = std::fs::remove_dir_all(&root);
}
