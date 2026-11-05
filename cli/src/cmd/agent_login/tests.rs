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

#[test]
fn declared_method_selects_the_agent_flow_and_may_take_a_stdin_key() {
    use clap::Parser;
    let login = |flags: &[&str]| {
        let mut args = vec!["domios", "agent", "login", "--tab", "owned"];
        args.extend(flags);
        crate::args::cli::Cli::try_parse_from(args)
    };
    assert!(login(&["--method", "opaque-method"]).is_ok());
    assert!(login(&["--method", "opaque-method", "--api-key-stdin"]).is_ok());
    for conflict in ["--device", "--cancel", "--show"] {
        assert!(login(&["--method", "opaque-method", conflict]).is_err());
    }
    assert_eq!(
        request_body(false, Some("opaque-method"), false),
        json!({"methodId":"opaque-method"})
    );
    assert_eq!(
        request_body(true, None, false),
        json!({"action":"start","method":"device"})
    );
    assert_eq!(
        request_body(false, Some("opaque-method"), true),
        json!({"action":"cancel"})
    );
}

#[test]
fn refusal_reports_the_daemon_reason_bounded() {
    assert_eq!(
        refusal(409, " Runtime sign-in is unsupported \n"),
        "Sign-in refused: Runtime sign-in is unsupported"
    );
    assert_eq!(
        refusal(409, ""),
        "Sign-in refused (HTTP 409). Check the tab's sign-in banner."
    );
    assert_eq!(
        refusal(409, &"x".repeat(2000)),
        format!("Sign-in refused: {}", "x".repeat(512))
    );
}

#[test]
fn show_lists_declared_methods_and_marks_key_methods() {
    let value = json!({"login":null,"authentication":{"state":"needs_auth","methods":[
        {"id":"opaque-browser","name":"Browser","available":true},
        {"id":"opaque-key","name":"Key","available":true},
        {"id":"opaque-gone","name":"Gone","available":false}]},
        "apiKeyMethods":["opaque-key"]});
    assert_eq!(
        render(&value, false),
        "Sign-in requested\nAuthentication: needs_auth\nDeclared sign-in methods (use --method ID):\n  opaque-browser  Browser\n  opaque-key  Key (API key via --api-key-stdin)\n  opaque-gone  Gone (unavailable here)\n"
    );
}
