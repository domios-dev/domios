use super::relaunch_env::filter_relaunch_env;

#[test]
fn filter_relaunch_env_strips_every_codeterm_variable_and_keeps_path_home() {
    let filtered = filter_relaunch_env([
        ("PATH", "/usr/bin"),
        ("HOME", "/Users/example"),
        ("USER", "example"),
        ("TMPDIR", "/tmp"),
        ("CARGO_TARGET_DIR", "/tmp/target"),
        ("CODETERM", "1"),
        ("CODETERM_INSTANCE", "aiderqa"),
        ("CODETERM_API_PORT", "7697"),
        ("CODETERM_API_URL", "http://127.0.0.1:7697"),
        ("CODETERM_API_TOKEN", "secret"),
        ("CODETERM_SOCKET_PATH", "/tmp/domios-dev-aiderqa.sock"),
        ("CODETERM_DATA_DIR", "/Users/example/.domios-dev-aiderqa"),
        ("CODETERM_DEV_IDENTIFIER", "com.codeterm.dev.aiderqa"),
        ("CODETERM_TAB_ID", "tab-1"),
        ("CODETERM_VITE_PORT", "1519"),
        ("CODETERM_CDP_PORT", "9322"),
    ]);

    assert_eq!(
        filtered,
        vec![
            ("PATH", "/usr/bin"),
            ("HOME", "/Users/example"),
            ("USER", "example"),
            ("TMPDIR", "/tmp"),
            ("CARGO_TARGET_DIR", "/tmp/target"),
        ]
    );
}

#[test]
fn filter_relaunch_env_keeps_nothing_instance_specific() {
    let filtered = filter_relaunch_env([
        ("HOME", "/home/example"),
        ("PATH", "/bin"),
        ("CODETERM", "1"),
        ("CODETERM_INSTANCE", "aiderqa"),
        ("CODETERM_DEV_IDENTIFIER", "com.codeterm.dev.aiderqa"),
        ("CODETERM_DATA_DIR", "/home/example/.domios-dev-aiderqa"),
        ("CODETERM_SOCKET_PATH", "/tmp/domios-dev-aiderqa.sock"),
        ("CODETERM_API_PORT", "7697"),
    ]);
    let keys: Vec<&str> = filtered.iter().map(|(key, _)| *key).collect();
    assert_eq!(keys, vec!["HOME", "PATH"]);
    assert!(filtered.iter().all(|(key, _)| {
        let upper = key.to_ascii_uppercase();
        upper != "CODETERM" && !upper.starts_with("CODETERM_")
    }));
}

#[test]
fn filter_relaunch_env_strips_codeterm_prefix_case_insensitively() {
    let filtered = filter_relaunch_env([
        ("Path", "/bin"),
        ("Home", "/home/example"),
        ("codeterm", "1"),
        ("codeterm_instance", "aiderqa"),
        ("CodeTerm_API_PORT", "7686"),
        ("CODETERM_tab_id", "tab-1"),
    ]);
    assert_eq!(filtered, vec![("Path", "/bin"), ("Home", "/home/example")]);
}

#[test]
fn filter_relaunch_env_keeps_unrelated_and_short_keys() {
    let filtered = filter_relaunch_env([
        ("C", "1"),
        ("CODETERM", "1"),
        ("CODETERM_", ""),
        ("CODETERMINAL", "x"),
        ("NOT_CODETERM_INSTANCE", "keep"),
    ]);
    assert_eq!(
        filtered,
        vec![
            ("C", "1"),
            ("CODETERMINAL", "x"),
            ("NOT_CODETERM_INSTANCE", "keep")
        ]
    );
}
