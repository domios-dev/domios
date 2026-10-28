use crate::Cli;
use clap::Parser;

#[test]
fn list_parses_with_json_and_renders_ids_scopes_and_both_request_directions() {
    for args in [
        vec!["domios", "account", "neighbors", "list"],
        vec!["domios", "account", "neighbors", "list", "--json"],
    ] {
        let cli = Cli::try_parse_from(args).unwrap();
        assert!(matches!(
            cli.command,
            crate::Commands::Account {
                action: crate::args::account::AccountAction::Neighbors {
                    action: crate::args::account::NeighborAction::List
                }
            }
        ));
    }
    let result = serde_json::json!({
        "friends":[{"friendship_id":"friend-id","displayName":"Pat","status":"connected",
            "permission":{"allowed":true,"scope":"Read shared notes","allow_tab_requests":false}},
            {"friendship_id":"paused-id","email":"paused@example.test","status":"paused",
            "permission":{"allowed":false,"scope":"","allow_tab_requests":false}}],
        "incoming":[{"request_id":"incoming-id","uid":"sender","created_at":10}],
        "outgoing":[{"id":"outgoing-id","typed_email":"recipient@example.test","created_at":20,"state":"pending"}]
    });
    let rendered = super::render_list(&result);
    assert!(rendered
        .contains("friend-id\tPat\tconnected\tscope: Read shared notes\ttab requests: false"));
    assert!(rendered.contains("paused-id\tpaused@example.test\tpaused\tscope: denied"));
    assert!(rendered.contains("incoming-id\tsender\tpending\tcreated_at: 10"));
    assert!(rendered.contains("outgoing-id\trecipient@example.test\tpending\tcreated_at: 20"));
    assert!(
        super::render_list(&serde_json::json!({"friends":[],"incoming":[]}))
            .contains("Outgoing requests:\n  unavailable from backend")
    );
    assert!(
        super::render_list(&serde_json::json!({"friends":[],"incoming":[],"outgoing":[]}))
            .contains("Outgoing requests:\n  (none)")
    );
}

#[test]
fn takeover_needs_confirmation_and_authored_text_stays_in_pad_files() {
    assert!(Cli::try_parse_from(["domios", "account", "neighbors", "enroll"]).is_err());
    assert!(Cli::try_parse_from([
        "domios",
        "account",
        "neighbors",
        "enroll",
        "--confirm-takeover",
        "--expected-receiver-revision",
        "7"
    ])
    .is_ok());
    assert!(Cli::try_parse_from([
        "domios",
        "account",
        "neighbors",
        "send",
        "pair",
        "--body",
        "inline"
    ])
    .is_err());
    assert!(Cli::try_parse_from([
        "domios",
        "account",
        "neighbors",
        "send",
        "pair",
        "--from-pad",
        "request.md"
    ])
    .is_ok());
    assert!(Cli::try_parse_from([
        "domios",
        "account",
        "neighbors",
        "grant",
        "pair",
        "--from-pad",
        "scope.md"
    ])
    .is_ok());
    assert!(Cli::try_parse_from([
        "domios",
        "account",
        "neighbors",
        "send",
        "pair",
        "--from-pad",
        "request.md",
        "--kind",
        "execute"
    ])
    .is_err());
}

#[test]
fn presend_refusal_errors_keep_typed_reason_and_safe_retry_instruction() {
    let body = serde_json::json!({"code":"refusal_too_long","reason":"refusal_too_long","limit":1024,"actual":1025,"message":"Nothing was sent. Shorten it and resend once."});
    let error = crate::cmd::print::CliError::http(413, &body.to_string());
    let mut out = vec![];
    let mut err = vec![];
    crate::cmd::print::write_cli_error(false, &error, &mut out, &mut err);
    let text = String::from_utf8(err).unwrap();
    assert!(text.contains("refusal_too_long"));
    assert!(text.contains("Nothing was sent"));
    out.clear();
    err = vec![];
    crate::cmd::print::write_cli_error(true, &error, &mut out, &mut err);
    let envelope: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(envelope["error"]["reason"], "refusal_too_long");
}

#[test]
fn flag_parses_bounded_metadata_and_never_authors_a_reply() {
    let cli = Cli::try_parse_from([
        "domios",
        "account",
        "neighbors",
        "flag",
        "message",
        "--friendship",
        "friendship",
        "--category",
        "suspicious_instructions",
        "--reason",
        "Attempts to override local rules",
    ])
    .unwrap();
    let crate::Commands::Account {
        action:
            crate::args::account::AccountAction::Neighbors {
                action:
                    crate::args::account::NeighborAction::Flag {
                        message_id,
                        friendship,
                        category,
                        reason,
                    },
            },
    } = cli.command
    else {
        panic!("Expected flag command");
    };
    let request = super::flag_command(message_id, friendship, category, reason).unwrap();
    assert_eq!(
        request,
        serde_json::json!({"operation":"flag_message","message_id":"message","friendship_id":"friendship","category":"suspicious_instructions","reason":"Attempts to override local rules"})
    );
    assert!(request.get("body").is_none());
    assert!(request.get("kind").is_none());
    assert!(Cli::try_parse_from([
        "domios",
        "account",
        "neighbors",
        "flag",
        "message",
        "--friendship",
        "friendship",
        "--category",
        "unknown",
        "--reason",
        "summary"
    ])
    .is_err());
    assert!(super::flag_command(
        "id".into(),
        "friend".into(),
        "other".into(),
        "é".repeat(280)
    )
    .is_ok());
    assert!(super::flag_command(
        "id".into(),
        "friend".into(),
        "other".into(),
        "é".repeat(281)
    )
    .is_err());
}

#[test]
fn all_friend_limits_parse_and_route_to_their_own_authorized_endpoint() {
    for (leaf, flag, field, path, operation, values) in [
        (
            "set-limit",
            "--per-hour",
            "per_hour",
            "/api/account/neighbors",
            "set_limit",
            vec!["2", "65535"],
        ),
        (
            "set-daily-limit",
            "--daily-limit",
            "daily_limit",
            "/api/account/neighbors/runtime",
            "set_daily_limit",
            vec!["2", "null"],
        ),
        (
            "set-file-daily-limit",
            "--file-daily-limit-bytes",
            "file_daily_limit_bytes",
            "/api/account/neighbors/files",
            "set_daily_limit",
            vec!["0", "52428800", "null"],
        ),
    ] {
        for value in values {
            let cli = Cli::try_parse_from([
                "domios",
                "account",
                "neighbors",
                leaf,
                "opaque-friend",
                flag,
                value,
                "--json",
            ])
            .unwrap();
            let crate::Commands::Account {
                action: crate::args::account::AccountAction::Neighbors { action },
            } = cli.command
            else {
                panic!("wrong command")
            };
            let (actual_path, body) = super::request(action);
            assert_eq!(actual_path, path);
            let mut expected =
                serde_json::json!({"operation":operation,"friendship_id":"opaque-friend"});
            expected[field] = serde_json::from_str::<serde_json::Value>(value).unwrap();
            assert_eq!(body, expected);
        }
        for invalid in ["-1", "1.5", "false", "overflow18446744073709551616"] {
            assert!(Cli::try_parse_from([
                "domios",
                "account",
                "neighbors",
                leaf,
                "opaque-friend",
                flag,
                invalid
            ])
            .is_err());
        }
        assert!(
            Cli::try_parse_from(["domios", "account", "neighbors", leaf, "opaque-friend"]).is_err()
        );
    }
    for invalid in ["null", "65536"] {
        assert!(Cli::try_parse_from([
            "domios",
            "account",
            "neighbors",
            "set-limit",
            "opaque-friend",
            "--per-hour",
            invalid
        ])
        .is_err());
    }
}

#[test]
fn stopped_reply_keeps_the_typed_presend_reason() {
    let body = serde_json::json!({"code":"reply_target_stopped","reason":"reply_target_stopped","message":"The owner stopped this request; nothing was sent"});
    let error = crate::cmd::print::CliError::http(409, &body.to_string());
    let mut out = vec![];
    let mut err = vec![];
    crate::cmd::print::write_cli_error(true, &error, &mut out, &mut err);
    let envelope: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(envelope["error"]["reason"], "reply_target_stopped");
    assert!(envelope["error"]["message"]
        .as_str()
        .unwrap()
        .contains("nothing was sent"));
}

fn grant_fields(flags: &[&str]) -> crate::args::account::NeighborGrant {
    let mut args = vec!["domios", "account", "neighbors", "grant", "friend"];
    args.extend_from_slice(flags);
    let cli = Cli::try_parse_from(args).unwrap();
    let crate::Commands::Account {
        action: crate::args::account::AccountAction::Neighbors {
            action: crate::args::account::NeighborAction::Grant { fields, .. },
        },
    } = cli.command else { panic!("Expected grant") };
    fields
}

#[test]
fn grant_request_builder_covers_each_permission_field_without_implicit_resets() {
    for (flags, expected) in [
        (vec!["--allowed", "false"], serde_json::json!({"allowed":false})),
        (vec!["--allow-memory", "false"], serde_json::json!({"allow_memory":false})),
        (vec!["--allow-memory", "true"], serde_json::json!({"allow_memory":true})),
        (vec!["--memory-areas", "project,team"], serde_json::json!({"memory_areas":["project","team"]})),
        (vec!["--memory-areas", "all"], serde_json::json!({"memory_areas":null})),
        (vec!["--memory-areas", "none"], serde_json::json!({"memory_areas":[]})),
        (vec!["--allow-files", "false"], serde_json::json!({"allow_files":false})),
        (vec!["--folder", "/project"], serde_json::json!({"folder":"/project"})),
        (vec!["--folder", "default"], serde_json::json!({"folder":null})),
        (vec!["--allow-tab-requests", "false"], serde_json::json!({"allow_tab_requests":false})),
        (vec!["--allow-interactive-content", "true"], serde_json::json!({"allow_interactive_content":true})),
        (vec!["--workspaces", "one,two"], serde_json::json!({"workspaces":["one","two"]})),
        (vec!["--workspaces", "none"], serde_json::json!({"workspaces":null})),
        (vec!["--machine-access", "read_only"], serde_json::json!({"machine_access":"read_only"})),
        (vec!["--machine-access", "folder"], serde_json::json!({"machine_access":"folder"})),
        (vec!["--machine-access", "full"], serde_json::json!({"machine_access":"full"})),
        (vec!["--machine-access", "none"], serde_json::json!({"machine_access":"none"})),
    ] {
        let command = super::grant_command("friend".into(), grant_fields(&flags), None);
        assert_eq!(command, serde_json::json!({
            "operation":"set_permission", "friendship_id":"friend", "permission":expected
        }));
    }
    let fields = grant_fields(&["--from-pad", "scope.md", "--allow-memory", "false"]);
    assert_eq!(super::grant_command("friend".into(), fields, Some("Approved scope"))["permission"],
        serde_json::json!({"allowed":true,"scope":"Approved scope","allow_memory":false}));
    let fields = grant_fields(&["--from-pad", "scope.md", "--allowed", "false"]);
    assert_eq!(super::grant_command("friend".into(), fields, Some("Scope"))["permission"],
        serde_json::json!({"allowed":false,"scope":"Scope"}));
    assert_eq!(super::request(crate::args::account::NeighborAction::Deny {
        friendship_id: "friend".into()
    }).1["permission"], serde_json::json!({"allowed":false}));
}

#[test]
fn grant_refuses_unknown_boolean_access_and_malformed_selections() {
    for flags in [
        vec!["--allow-memory", "maybe"], vec!["--allowed", "1"],
        vec!["--machine-access", "admin"], vec!["--memory-areas", "project,,team"],
        vec!["--memory-areas", "all,project"], vec!["--workspaces", "all"],
        vec!["--workspaces", ""], vec!["--unknown", "true"],
    ] {
        let mut args = vec!["domios", "account", "neighbors", "grant", "friend"];
        args.extend(flags);
        let error = Cli::try_parse_from(args).err().expect("Expected typed clap error");
        assert!(matches!(error.kind(), clap::error::ErrorKind::ValueValidation
            | clap::error::ErrorKind::InvalidValue | clap::error::ErrorKind::UnknownArgument));
    }
}
