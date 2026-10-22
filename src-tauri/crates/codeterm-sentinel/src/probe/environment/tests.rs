use super::*;

#[test]
fn probe_path_adds_homebrew_without_reordering_inherited_tools() {
    let inherited = std::env::join_paths(["/usr/bin", "/usr/local/bin"]).unwrap();
    let path = probe_path(Some(&inherited), &["/opt/homebrew/bin", "/usr/local/bin"]).unwrap();
    let entries: Vec<_> = std::env::split_paths(&path).collect();
    assert_eq!(
        entries,
        vec![
            std::path::PathBuf::from("/usr/bin"),
            std::path::PathBuf::from("/usr/local/bin"),
            std::path::PathBuf::from("/opt/homebrew/bin")
        ]
    );
}

#[test]
fn sentinel_probe_environment_strips_instance_credentials_and_cli_path() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("instance-bin");
    let inherited_cli = dir.path().join("inherited-bin");
    let normal = dir.path().join("normal-bin");
    let path = std::env::join_paths([bin.clone(), normal.clone(), inherited_cli.clone()]).unwrap();
    let inherited = vec![
        ("CODETERM_API_TOKEN".into(), "opaque-token".into()),
        ("CODETERM_API_URL".into(), "http://127.0.0.1:7685".into()),
        ("CODETERM_SOCKET_PATH".into(), "opaque-socket".into()),
        (
            "CODETERM_CLI_PATH".into(),
            inherited_cli.join("codeterm").into_os_string(),
        ),
        ("DOMIOS_API_TOKEN".into(), "opaque-domios-token".into()),
        ("PATH".into(), path),
        ("HOME".into(), dir.path().as_os_str().to_owned()),
        ("LANG".into(), "C.UTF-8".into()),
        ("TMP".into(), dir.path().as_os_str().to_owned()),
    ];
    let environment = Environment::for_command(
        inherited,
        Some(&bin.join("codeterm")),
        &[bin.to_str().unwrap()],
    );
    let variables: std::collections::BTreeMap<_, _> = environment.variables.into_iter().collect();
    assert_eq!(variables.len(), 4);
    assert_eq!(
        std::env::split_paths(&variables[OsStr::new("PATH")]).collect::<Vec<_>>(),
        vec![normal]
    );
    assert_eq!(variables[OsStr::new("HOME")], dir.path().as_os_str());
    assert_eq!(variables[OsStr::new("LANG")], "C.UTF-8");
    assert_eq!(variables[OsStr::new("TMP")], dir.path().as_os_str());
}

#[test]
fn sentinel_probe_environment_strips_all_mixed_case_instance_selectors() {
    let names = [
        "CODETERM",
        "DOMIOS",
        "CODETERM_API_TOKEN",
        "CODETERM_TAB_ID",
        "CODETERM_API_PORT",
        "CODETERM_INSTANCE",
        "CODETERM_DATA_DIR",
        "CODETERM_CONFIG_DIR",
        "CODETERM_FUTURE_SECRET",
        "DoMiOs_ApI_Url",
        "domios_socket_path",
        "codeterm_future_secret",
    ];
    let inherited = names
        .into_iter()
        .map(|key| (OsString::from(key), OsString::from("opaque")));
    let environment = Environment::for_command(inherited, None, &[]);
    assert!(environment.variables.is_empty());
    #[cfg(windows)]
    assert_eq!(environment.windows_block(), vec![0, 0]);
}

#[cfg(windows)]
#[test]
fn sentinel_probe_environment_windows_block_contains_only_filtered_snapshot() {
    let environment = Environment::for_command(
        [
            ("CODETERM_API_TOKEN".into(), "opaque-token".into()),
            ("DOMIOS_INSTANCE".into(), "opaque-instance".into()),
            ("HOME".into(), "opaque-home".into()),
        ],
        None,
        &[],
    );
    assert_eq!(
        String::from_utf16(&environment.windows_block()).unwrap(),
        "HOME=opaque-home\0\0"
    );
}
