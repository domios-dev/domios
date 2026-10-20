use super::*;

#[test]
fn launch_environment_summary_keeps_credentials_secret_free() {
    let token = "launch-token-fixture";
    let nonce = "launch-nonce-fixture";
    let launch_env = std::collections::HashMap::from([
        (
            "CODETERM_API_URL".to_string(),
            "http://127.0.0.1:7686/private?token=hidden".to_string(),
        ),
        ("CODETERM_TAB_ID".to_string(), "tab-1".to_string()),
        ("CODETERM_API_TOKEN".to_string(), token.to_string()),
        (
            "CODETERM_SESSION_BINDING_NONCE".to_string(),
            nonce.to_string(),
        ),
    ]);
    let inherited_env = std::collections::HashMap::new();

    let summary = summarize_launch_environment("tab-1", &launch_env, &inherited_env, None);
    assert_eq!(summary.api_target, "127.0.0.1:7686");
    assert_eq!(summary.api_target_source, "launch");
    assert!(summary.tab_id_present);
    assert!(summary.tab_id_matches);
    assert!(summary.token_present);
    assert!(summary.binding_nonce_present);
    let rendered = format!("{summary:?}");
    assert!(!rendered.contains(token));
    assert!(!rendered.contains(nonce));
    assert!(!rendered.contains("hidden"));
}

#[test]
fn launch_environment_summary_reports_path_facts_without_values() {
    let launch_env = std::collections::HashMap::from([
        (
            "Path".to_string(),
            std::env::join_paths([
                std::path::PathBuf::from("root").join("System32"),
                std::path::PathBuf::from("private-bin"),
                std::path::PathBuf::new(),
            ])
            .unwrap()
            .to_string_lossy()
            .to_string(),
        ),
        ("ComSpec".to_string(), "cmd".into()),
    ]);
    let inherited_env = std::collections::HashMap::new();

    let summary = summarize_launch_environment("tab-1", &launch_env, &inherited_env, None);
    assert!(summary.path_system32_present);
    assert!(summary.comspec_present);
    assert_eq!(summary.path_entry_count, 2);
    let rendered = format!("{summary:?}");
    assert!(!rendered.contains("root"));
    assert!(!rendered.contains("private-bin"));
}

#[test]
fn launch_environment_summary_counts_the_prepended_cli_directory() {
    let cli_dir = std::path::PathBuf::from("codeterm-bin");
    let launch_env = std::collections::HashMap::from([(
        "PATH".into(),
        std::env::join_paths([
            std::path::PathBuf::from("System32"),
            std::path::PathBuf::from("provider-bin"),
        ])
        .unwrap()
        .to_string_lossy()
        .to_string(),
    )]);
    let inherited_env = std::collections::HashMap::new();

    let summary =
        summarize_launch_environment("tab-1", &launch_env, &inherited_env, Some(&cli_dir));

    assert_eq!(summary.path_entry_count, 3);
}

#[test]
fn launch_environment_summary_resolves_path_casing_deterministically() {
    let canonical_path = std::env::join_paths([
        std::path::PathBuf::from("root/System32"),
        std::path::PathBuf::from("canonical-bin"),
    ])
    .unwrap()
    .to_string_lossy()
    .to_string();
    let alias_path = std::env::join_paths([std::path::PathBuf::from("alias-bin")])
        .unwrap()
        .to_string_lossy()
        .to_string();
    let launch_env = std::collections::HashMap::from([
        ("Path".into(), alias_path),
        ("PATH".into(), canonical_path),
    ]);
    let inherited_env = std::collections::HashMap::new();

    let summary = summarize_launch_environment("tab-1", &launch_env, &inherited_env, None);

    assert!(summary.path_system32_present);
    assert_eq!(summary.path_entry_count, 2);
}

#[test]
fn launch_environment_summary_uses_effective_path_override() {
    let launch_env = std::collections::HashMap::from([
        ("PATH".into(), String::new()),
        ("COMSPEC".into(), String::new()),
    ]);
    let inherited_env = std::collections::HashMap::from([
        ("Path".into(), r"C:\Windows\System32;C:\bin".into()),
        ("ComSpec".into(), r"C:\Windows\System32\cmd.exe".into()),
    ]);

    let summary = summarize_launch_environment("tab-1", &launch_env, &inherited_env, None);
    assert!(!summary.path_system32_present);
    assert!(!summary.comspec_present);
    assert_eq!(summary.path_entry_count, 0);
}

#[test]
fn launch_environment_summary_reads_inherited_path_and_comspec() {
    let launch_env = std::collections::HashMap::new();
    let inherited_env = std::collections::HashMap::from([
        (
            "Path".into(),
            std::env::join_paths([
                std::path::PathBuf::from("root").join("System32"),
                std::path::PathBuf::from("inherited-bin"),
            ])
            .unwrap()
            .to_string_lossy()
            .to_string(),
        ),
        ("ComSpec".into(), "command-processor".into()),
    ]);

    let summary = summarize_launch_environment("tab-1", &launch_env, &inherited_env, None);
    assert!(summary.path_system32_present);
    assert!(summary.comspec_present);
    assert_eq!(summary.path_entry_count, 2);
    let rendered = format!("{summary:?}");
    assert!(!rendered.contains("inherited-bin"));
    assert!(!rendered.contains("command-processor"));
}

#[test]
fn api_target_keeps_only_host_and_effective_port() {
    assert_eq!(
        api_target(Some("https://api.example.test/path?token=hidden")),
        "api.example.test:443",
    );
    assert_eq!(api_target(Some("http://[::1]:7686/private")), "[::1]:7686",);
}

#[test]
fn launch_environment_summary_identifies_inherited_credentials_without_values() {
    let launch_env = std::collections::HashMap::from([(
        "CODETERM_API_URL".to_string(),
        "http://127.0.0.1:7686".to_string(),
    )]);
    let inherited_env = std::collections::HashMap::from([
        (
            "CODETERM_API_TOKEN".to_string(),
            "parent-token-fixture".to_string(),
        ),
        (
            "CODETERM_SESSION_BINDING_NONCE".to_string(),
            "parent-nonce-fixture".to_string(),
        ),
    ]);

    let summary = summarize_launch_environment("tab-1", &launch_env, &inherited_env, None);
    assert!(summary.token_present);
    assert_eq!(summary.token_source, "inherited");
    assert!(summary.binding_nonce_present);
    assert_eq!(summary.binding_nonce_source, "inherited");
    let rendered = format!("{summary:?}");
    assert!(!rendered.contains("parent-token-fixture"));
    assert!(!rendered.contains("parent-nonce-fixture"));
}

#[test]
fn launch_environment_summary_marks_missing_and_mismatched_values() {
    let launch_env = std::collections::HashMap::from([
        ("CODETERM_API_URL".to_string(), "not-a-url".to_string()),
        ("CODETERM_TAB_ID".to_string(), "other-tab".to_string()),
        ("CODETERM_API_TOKEN".to_string(), " ".to_string()),
    ]);
    let inherited_env = std::collections::HashMap::new();

    let summary = summarize_launch_environment("tab-1", &launch_env, &inherited_env, None);
    assert_eq!(summary.api_target, "invalid");
    assert_eq!(summary.api_target_source, "launch");
    assert!(summary.tab_id_present);
    assert!(!summary.tab_id_matches);
    assert!(!summary.token_present);
    assert!(!summary.binding_nonce_present);
}

#[test]
fn redacts_common_secret_markers() {
    let input = "CODETERM_API_TOKEN=abc123; curl -H \"Authorization: Bearer xyz789\"";
    let got = redact_secrets(input);
    assert!(!got.contains("abc123"));
    assert!(!got.contains("xyz789"));
    assert!(got.contains("CODETERM_API_TOKEN=<redacted>"));
    assert!(got.contains("Authorization: Bearer <redacted>"));
}

#[test]
fn tail_bytes_preserves_utf8_boundary() {
    let input = format!("{}end", "привет".repeat(100));
    let got = tail_bytes(&input, 17);
    assert!(got.ends_with("end"));
    assert!(got.is_char_boundary(0));
}

#[test]
fn launch_digest_never_contains_source_text() {
    let source = "private launch prompt";
    let got = digest(source);
    assert_eq!(got.len(), 64);
    assert!(!got.contains(source));
}
