use super::*;
use crate::{Cli, Commands};
use clap::Parser;

#[test]
fn parses_request_status_cancel_and_never_accepts_secret_value_flags() {
    let parsed = Cli::try_parse_from([
        "domios",
        "secret",
        "request",
        "--machine",
        "rollacodebot",
        "--target",
        "vault:github-pat",
        "--kind",
        "token",
        "--reason",
        "Push release tag from cloud worker",
    ])
    .expect("secret request should parse");
    match parsed.command {
        Commands::Secret {
            action:
                SecretAction::Request {
                    machine,
                    target,
                    kind,
                    reason,
                    ..
                },
        } => {
            assert_eq!(machine.as_deref(), Some("rollacodebot"));
            assert_eq!(target, "vault:github-pat");
            assert_eq!(kind, SecretInputKindArg::Token);
            assert_eq!(reason, "Push release tag from cloud worker");
        }
        _ => panic!("expected Commands::Secret(Request)"),
    }

    let status = Cli::try_parse_from(["domios", "secret", "status", "sir_abc123"])
        .expect("secret status should parse");
    assert!(matches!(
        status.command,
        Commands::Secret {
            action: SecretAction::Status { .. }
        }
    ));

    let cancel = Cli::try_parse_from(["domios", "secret", "cancel", "sir_abc123"])
        .expect("secret cancel should parse");
    assert!(matches!(
        cancel.command,
        Commands::Secret {
            action: SecretAction::Cancel { .. }
        }
    ));

    for forbidden in ["--value", "--code", "--password", "--stdin"] {
        let err = Cli::try_parse_from([
            "domios",
            "secret",
            "request",
            "--target",
            "vault:test",
            "--kind",
            "token",
            "--reason",
            "Test",
            forbidden,
            "secret-leak",
        ]);
        assert!(
            err.is_err(),
            "domios secret request must never accept {forbidden}"
        );
    }
}

#[test]
fn validates_closed_target_syntax_and_renders_status_without_secrets() {
    assert!(validate_target_syntax("vault:deploy-key").is_ok());
    assert!(validate_target_syntax("provider-login:tab-1").is_ok());
    assert!(validate_target_syntax("provider-key:anthropic").is_ok());
    assert!(validate_target_syntax("shell:tab-1").is_err());
    assert!(validate_target_syntax("vault:").is_err());

    let rendered = render_human(&serde_json::json!({
        "requestId": "sir_123",
        "status": "pending",
        "targetMachineName": "rollacodebot",
        "targetLabel": "vault:deploy-key",
        "kind": "token",
    }));
    assert!(rendered.contains("sir_123"));
    assert!(rendered.contains("pending"));
    assert!(rendered.contains("rollacodebot"));
}
