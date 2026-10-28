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
