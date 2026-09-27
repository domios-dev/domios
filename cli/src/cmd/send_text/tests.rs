use super::{
    analyze_send_text_response, classify_tab_kind, decide_send_eligibility,
    guard_against_shell_mangle, is_task_shaped_payload, key_action_bytes, parse_actions_spec,
    parse_tab_targets, resolve_send_text_input, SendActionSpec, SendEligibility, SendTextOutcome,
    TabKind,
};

const CHARTER: &str = "You are Worker Leo.\nExecute the task; report on completion.";

#[test]
fn classify_agent_tab_from_agent_type() {
    let record = serde_json::json!({"id": "t1", "agent_type": "claude", "backing_kind": "pty"});
    assert_eq!(classify_tab_kind(&record), TabKind::Agent);
}

#[test]
fn send_after_adopt_delivers_prose_as_agent_not_shell() {
    let adopted = serde_json::json!({"id": "t1", "agent_type": "mock-pty", "backing_kind": "pty"});
    assert_eq!(classify_tab_kind(&adopted), TabKind::Agent);
    assert_eq!(
        decide_send_eligibility(TabKind::Agent, CHARTER, false, false),
        SendEligibility::Allow
    );
    match decide_send_eligibility(TabKind::RawShell, CHARTER, false, false) {
        SendEligibility::Refuse(msg) => assert!(msg.contains("--to-shell")),
        other => panic!("raw shell must still refuse prose, got {other:?}"),
    }
}

#[test]
fn classify_raw_shell_when_pty_without_agent() {
    let record = serde_json::json!({"id": "t1", "agent_type": null, "backing_kind": "pty"});
    assert_eq!(classify_tab_kind(&record), TabKind::RawShell);
    let empty = serde_json::json!({"id": "t1", "agent_type": "", "backing_kind": "pty"});
    assert_eq!(classify_tab_kind(&empty), TabKind::RawShell);
}

#[test]
fn classify_chat_backend_and_unknown() {
    let chat = serde_json::json!({"id": "t1", "backing_kind": "chat_backend"});
    assert_eq!(classify_tab_kind(&chat), TabKind::ChatBackend);
    let unknown = serde_json::json!({"id": "t1"});
    assert_eq!(classify_tab_kind(&unknown), TabKind::Unknown);
}

#[test]
fn task_shaped_payload_detects_multiline_and_long() {
    assert!(is_task_shaped_payload("first line\nsecond line"));
    assert!(is_task_shaped_payload(&"x".repeat(241)));
    assert!(!is_task_shaped_payload("git status"));
    assert!(!is_task_shaped_payload(&"y".repeat(240)));
}

#[test]
fn agent_tab_allows_prose() {
    assert_eq!(
        decide_send_eligibility(TabKind::Agent, CHARTER, false, false),
        SendEligibility::Allow
    );
}

#[test]
fn raw_shell_refuses_prose_by_default() {
    match decide_send_eligibility(TabKind::RawShell, CHARTER, false, false) {
        SendEligibility::Refuse(msg) => {
            assert!(msg.contains("raw shell"));
            assert!(msg.contains("--to-shell"));
        }
        other => panic!("expected Refuse, got {:?}", other),
    }
}

#[test]
fn raw_shell_allows_short_command() {
    assert_eq!(
        decide_send_eligibility(TabKind::RawShell, "git status", false, false),
        SendEligibility::Allow
    );
}

#[test]
fn to_shell_override_allows_prose_to_shell() {
    assert_eq!(
        decide_send_eligibility(TabKind::RawShell, CHARTER, false, true),
        SendEligibility::Allow
    );
}

#[test]
fn require_agent_refuses_non_agent_targets() {
    for kind in [TabKind::RawShell, TabKind::ChatBackend, TabKind::Unknown] {
        assert!(
            matches!(
                decide_send_eligibility(kind, "git status", true, false),
                SendEligibility::Refuse(_)
            ),
            "require_agent must refuse {:?}",
            kind
        );
    }
    assert_eq!(
        decide_send_eligibility(TabKind::Agent, "git status", true, false),
        SendEligibility::Allow
    );
}

#[test]
fn chat_backend_allows_prose_by_default() {
    assert_eq!(
        decide_send_eligibility(TabKind::ChatBackend, CHARTER, false, false),
        SendEligibility::Allow
    );
}

#[test]
fn send_reports_pending_or_refused_never_sent_when_nothing_written() {
    let v = serde_json::json!({ "ok": true });
    match analyze_send_text_response(&v) {
        SendTextOutcome::Pending {
            message,
            bytes_written,
        } => {
            assert_eq!(message, "OK (pending)");
            assert!(!bytes_written);
        }
        other => panic!("expected Pending, got {:?}", other),
    }

    let refusal = serde_json::json!({
        "ok": false,
        "error": { "code": "tab_busy_refused", "message": "nothing was written" }
    });
    match analyze_send_text_response(&refusal) {
        SendTextOutcome::Error { msg, exit_code } => {
            assert_eq!(exit_code, 2);
            assert_eq!(msg, "nothing was written");
        }
        other => panic!("expected explicit refusal, got {:?}", other),
    }

    let sent = serde_json::json!({ "ok": true, "delivery": "sent" });
    assert_eq!(
        analyze_send_text_response(&sent),
        SendTextOutcome::Success("OK (sent)".into()),
    );
}

#[test]
fn pending_route_result_is_not_rendered_as_sent() {
    let pending = serde_json::json!({
        "ok": true,
        "delivery": "pending",
        "bytes_written": false
    });
    assert_eq!(
        analyze_send_text_response(&pending),
        SendTextOutcome::Pending {
            message: "OK (pending)".into(),
            bytes_written: false,
        },
    );
}

#[test]
fn unconfirmed_write_stays_pending_but_preserves_written_evidence() {
    let pending = serde_json::json!({
        "ok": true,
        "delivery": "pending",
        "bytes_written": true
    });
    assert_eq!(
        analyze_send_text_response(&pending),
        SendTextOutcome::Pending {
            message: "OK (pending)".into(),
            bytes_written: true,
        },
    );

    let unconfirmed = serde_json::json!({
        "ok": false,
        "error": { "code": "submit_unconfirmed", "bytes_written": true }
    });
    assert_eq!(
        analyze_send_text_response(&unconfirmed),
        SendTextOutcome::Pending {
            message: "OK (pending)".into(),
            bytes_written: true,
        },
    );
}

#[test]
fn explicit_error_field_exits_two() {
    let v = serde_json::json!({
        "ok": false,
        "error": "PTY closed mid-write",
    });
    match analyze_send_text_response(&v) {
        SendTextOutcome::Error { msg, exit_code } => {
            assert_eq!(exit_code, 2);
            assert!(msg.contains("PTY closed"));
        }
        other => panic!("expected Error, got {:?}", other),
    }
}

#[test]
fn legitimate_text_passes() {
    assert!(guard_against_shell_mangle("hello world").is_ok());
    assert!(guard_against_shell_mangle("PR #28 — looks fine").is_ok());
    assert!(guard_against_shell_mangle("status code: 200").is_ok());
}

#[test]
fn bash_command_not_found_rejected() {
    let payload = "/usr/bin/bash: line 3: mesh: command not found\nactual content";
    assert!(guard_against_shell_mangle(payload).is_err());
}

#[test]
fn bare_command_not_found_rejected() {
    let payload = "mesh: command not found\nfollowed by intended message";
    assert!(guard_against_shell_mangle(payload).is_err());
}

#[test]
fn syntax_error_rejected() {
    let payload = "bash: -c: line 1: syntax error near unexpected token `('";
    assert!(guard_against_shell_mangle(payload).is_err());
}

#[test]
fn guard_ignores_marker_inside_fenced_code_block() {
    let payload = "Here is the repro:\n```\npunset: command not found\n```\nEnd.";
    assert!(guard_against_shell_mangle(payload).is_ok());
}

#[test]
fn guard_still_fires_on_marker_outside_code_block() {
    let payload = "this is just /usr/bin/bash: line 5: foo not in a code block";
    assert!(guard_against_shell_mangle(payload).is_err());
}

#[test]
fn guard_handles_unterminated_fence_safely() {
    // Unterminated fence treats the rest as code-block — better to skip a potentially-spurious guard than reject legitimate content.
    let payload = "```\n: command not found\nstill open\n";
    assert!(guard_against_shell_mangle(payload).is_ok());
}

#[test]
fn guard_with_multiple_fences_strips_only_inside() {
    let payload =
        "outside ok\n```\n: command not found\n```\nbetween\n```\nbash: line 1: x\n```\ntrailing";
    assert!(guard_against_shell_mangle(payload).is_ok());
}

#[test]
fn stdin_resolution_preserves_quotes_and_trailing_newline() {
    let input = "it's got 'quotes' and \"doubles\"\nsecond line\n";
    let mut stdin = input.as_bytes();
    let (text, source) = resolve_send_text_input(None, None, None, true, &mut stdin).unwrap();
    assert_eq!(source, "--stdin");
    assert_eq!(text, input);
}

#[test]
fn action_parser_accepts_keys_delays_and_repeats() {
    let parsed = parse_actions_spec("esc,delay:200ms,down*3,enter").unwrap();
    assert_eq!(
        parsed,
        vec![
            SendActionSpec::Key("esc".into()),
            SendActionSpec::Delay(200),
            SendActionSpec::Key("down".into()),
            SendActionSpec::Key("down".into()),
            SendActionSpec::Key("down".into()),
            SendActionSpec::Key("enter".into()),
        ]
    );
}

#[test]
fn action_parser_accepts_ctrl_keys() {
    assert_eq!(key_action_bytes("ctrl-c").unwrap(), vec![3]);
    assert_eq!(
        parse_actions_spec("ctrl-c").unwrap(),
        vec![SendActionSpec::Key("ctrl-c".into())]
    );
}

#[test]
fn actions_parser_accepts_literal_character_keys() {
    assert_eq!(
        parse_actions_spec("y,n,A").unwrap(),
        vec![
            SendActionSpec::Key("y".into()),
            SendActionSpec::Key("n".into()),
            SendActionSpec::Key("A".into()),
        ]
    );
}

#[test]
fn action_parser_rejects_unknown_keys() {
    assert!(parse_actions_spec("launch-missiles").is_err());
}

#[test]
fn composite_tab_id_routes_over_mesh() {
    use crate::cmd::tab::remote_target::parse_remote_tab_id;
    assert_eq!(
        parse_remote_tab_id("mesh:BLACKSTAR/0e39ec9e"),
        Some(("BLACKSTAR".into(), "0e39ec9e".into()))
    );
    assert!(parse_remote_tab_id("0e39ec9e").is_none());
}

#[test]
fn remote_send_body_carries_text_and_source() {
    let body = super::remote_send_body("deliver this", None, Some("af5919cb"));
    assert_eq!(body["text"], serde_json::json!("deliver this"));
    assert_eq!(body["source_tab"], serde_json::json!("af5919cb"));
    assert!(body.get("actions").is_none());
}

#[test]
fn remote_send_body_omits_empty_text_and_blank_source() {
    let actions = parse_actions_spec("enter").unwrap();
    let body = super::remote_send_body("", Some(&actions), Some(""));
    assert!(body.get("text").is_none());
    assert!(body.get("source_tab").is_none());
    assert_eq!(body["actions"], super::actions_to_json(&actions));
}

/// The resolver is the single seam every coordination send shares, so the
/// refusal has to land there, before a tab id is even resolved.
#[test]
fn quoted_handoff_inline_payload_is_refused_by_the_send_resolver() {
    let authored = "Goal: `plan handoff` must arrive byte-exact.\nRules: no $VARS lost.";
    let err = resolve_send_text_input(
        Some(authored.to_string()),
        None,
        None,
        false,
        &mut std::io::empty(),
    )
    .expect_err("an inline authored send is refused");
    assert!(err.starts_with(crate::cmd::quoted_handoff::REFUSAL_CODE));
    assert!(err.contains("--from-pad"));
}

#[test]
fn quoted_handoff_scalar_send_and_staged_sources_still_resolve() {
    let (text, source) = resolve_send_text_input(
        Some("status?".to_string()),
        None,
        None,
        false,
        &mut std::io::empty(),
    )
    .expect("a scalar argument stays inline");
    assert_eq!((text.as_str(), source), ("status?", "positional"));

    let staged = "Goal: `plan handoff` must arrive byte-exact.\nRules: no $VARS lost.\n";
    let (text, source) = resolve_send_text_input(
        None,
        None,
        None,
        true,
        &mut std::io::Cursor::new(staged.as_bytes().to_vec()),
    )
    .expect("a staged payload is never refused for its shape");
    assert_eq!((text.as_str(), source), (staged, "--stdin"));
}

#[test]
fn multi_tab_send_parses_trims_and_dedupes_in_order() {
    assert_eq!(
        parse_tab_targets(" a , b ,a, c ").expect("list parses"),
        vec!["a".to_string(), "b".to_string(), "c".to_string()],
        "entries trim, duplicates collapse, first-seen order survives"
    );
}

#[test]
fn multi_tab_send_single_id_yields_exactly_one_target() {
    assert_eq!(
        parse_tab_targets("solo").expect("single id parses"),
        vec!["solo".to_string()],
        "a comma-less value must stay a one-target list so the single-tab path is unchanged"
    );
}

#[test]
fn multi_tab_send_rejects_empty_entries() {
    for spec in ["a,,b", "a,", ",b", " , "] {
        let err = parse_tab_targets(spec).expect_err("empty entry must be refused");
        assert!(
            err.contains("empty entry"),
            "refusal names the defect for {spec}: {err}"
        );
    }
}
