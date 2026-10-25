use super::*;
#[test]
fn login_cli_accepts_stdin_and_refuses_argv_credentials() {
    use clap::Parser;
    for flags in [
        vec![],
        vec!["--device"],
        vec!["--show", "--mesh", "opaque-peer", "--json"],
        vec!["--api-key-stdin"],
        vec!["--cancel"],
    ] {
        let mut args = vec!["codeterm", "agent", "login", "--tab", "fixture"];
        args.extend(flags);
        assert!(crate::args::cli::Cli::try_parse_from(args).is_ok());
    }
    assert!(crate::args::cli::Cli::try_parse_from([
        "codeterm",
        "agent",
        "login",
        "--tab",
        "fixture",
        "--api-key",
        "fixture"
    ])
    .is_err());
    assert!(crate::args::cli::Cli::try_parse_from([
        "codeterm",
        "agent",
        "login",
        "--tab",
        "fixture",
        "--api-key-stdin",
        "--device"
    ])
    .is_err());
}
#[test]
fn api_key_input_is_bounded_and_trimmed() {
    assert_eq!(
        read_key(&b" opaque-fixture\n"[..]).unwrap(),
        "opaque-fixture"
    );
    assert!(read_key(&b"\n"[..]).is_err());
    assert!(read_key(vec![b'x'; 16385].as_slice()).is_err());
}

#[test]
fn login_output_preserves_url_code_and_expiry_in_text_and_json() {
    let value = json!({"login":{"status":"pending","url":"https://example.invalid/opaque?x=2&y=3","userCode":"opaque-token","expiresAtMs":5000}});
    assert_eq!(render(&value,false),"Sign-in pending\nOpen in your system browser: https://example.invalid/opaque?x=2&y=3\nDevice code: opaque-token\nExpires at (Unix milliseconds): 5000\n");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&render(&value, true)).unwrap(),
        value
    );
}

#[test]
fn login_show_refuses_mutating_flags() {
    use clap::Parser;
    for flag in ["--device", "--cancel", "--api-key-stdin"] {
        assert!(crate::args::cli::Cli::try_parse_from([
            "domios", "agent", "login", "--tab", "owned", "--show", flag
        ])
        .is_err());
    }
}
