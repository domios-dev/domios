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
    assert_eq!(variables.len(), 2);
    assert_eq!(
        std::env::split_paths(&variables[OsStr::new("PATH")]).collect::<Vec<_>>(),
        vec![normal]
    );
    assert!(!variables.contains_key(OsStr::new("HOME")));
    assert_eq!(variables[OsStr::new("LANG")], "C.UTF-8");
    assert!(!variables.contains_key(OsStr::new("TMP")));
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
    assert_eq!(environment.variables.len(), 2);
    assert!(environment
        .variables
        .iter()
        .all(|(key, _)| key == "PATH" || key == "LANG"));
    #[cfg(windows)]
    assert!(String::from_utf16(&environment.windows_block())
        .unwrap()
        .contains("LANG=C.UTF-8"));
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
        "LANG=C.UTF-8\0PATH=\0\0"
    );
}

#[test]
fn blueprint_v2_runtime_command_environment_is_an_allowlist() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join(if cfg!(windows) {
            "domios.exe"
        } else {
            "domios"
        }),
        b"opaque-cli",
    )
    .unwrap();
    let environment = Environment::for_command(
        [
            ("OPENAI_API_KEY".into(), "fake-openai-key".into()),
            ("AWS_SECRET_ACCESS_KEY".into(), "fake-aws-key".into()),
            ("GH_TOKEN".into(), "fake-gh-token".into()),
            ("SSH_AUTH_SOCK".into(), "fake-agent-socket".into()),
            ("NODE_OPTIONS".into(), "--require=credential-loader".into()),
            ("BASH_ENV".into(), "credential-loader".into()),
            ("CODETERM_API_TOKEN".into(), "fake-instance-key".into()),
            ("PATH".into(), directory.path().as_os_str().to_owned()),
            (
                "DOMIOS_CLI_PATH".into(),
                directory.path().join("domios").into_os_string(),
            ),
        ],
        None,
        &[directory.path().to_str().unwrap()],
    );
    assert!(environment
        .variables
        .iter()
        .all(|(key, _)| key == "LANG" || key == "PATH"));
    let (_, path) = environment
        .variables
        .iter()
        .find(|(key, _)| key == "PATH")
        .unwrap();
    assert!(!std::env::split_paths(path).any(|path| path == directory.path()));
}
