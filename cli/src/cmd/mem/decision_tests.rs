use super::decision::{read_token, Reply};
use super::socket::socket_request;
use crate::{
    Cli, Commands, DecisionRerankMode, MemAction, MemDecisionAction, MemDecisionTokenAction,
};
use clap::Parser;
use serde_json::json;

fn parse_mem(argv: &[&str]) -> Result<MemAction, clap::Error> {
    let mut full = vec!["codeterm", "mem"];
    full.extend_from_slice(argv);
    match Cli::try_parse_from(full)?.command {
        Commands::Mem { action } => Ok(action),
        _ => panic!("expected mem command"),
    }
}

fn search_mode(argv: &[&str]) -> Option<DecisionRerankMode> {
    match parse_mem(argv).unwrap() {
        MemAction::Search {
            decision_rerank, ..
        } => decision_rerank,
        _ => panic!("expected mem search"),
    }
}

#[test]
fn mem_decision_search_flag_parses_on_off() {
    assert_eq!(
        search_mode(&["search", "q", "--decision-rerank", "on"]),
        Some(DecisionRerankMode::On)
    );
    assert_eq!(
        search_mode(&["search", "q", "--decision-rerank", "off"]),
        Some(DecisionRerankMode::Off)
    );
    assert_eq!(search_mode(&["search", "q"]), None);
    assert!(parse_mem(&["search", "q", "--decision-rerank", "maybe"]).is_err());
}

#[test]
fn mem_decision_search_request_omits_key_without_flag() {
    let (method, params) = socket_request(&parse_mem(&["search", "q"]).unwrap()).unwrap();
    assert_eq!(method, "mem.search");
    let params = params.unwrap();
    assert!(params.as_object().unwrap().get("decision_rerank").is_none());

    for (flag, expected) in [("on", true), ("off", false)] {
        let action = parse_mem(&["search", "q", "--decision-rerank", flag]).unwrap();
        let (_, params) = socket_request(&action).unwrap();
        assert_eq!(params.unwrap()["decision_rerank"], json!(expected));
    }
}

#[test]
fn mem_decision_token_set_has_no_value_flag() {
    for flag in ["--token", "--value", "--password"] {
        assert!(
            parse_mem(&["decision", "token", "set", flag, "x"]).is_err(),
            "{flag} must be rejected"
        );
    }
    match parse_mem(&["decision", "token", "set"]).unwrap() {
        MemAction::Decision {
            action:
                MemDecisionAction::Token {
                    action: MemDecisionTokenAction::Set { adapter },
                },
        } => assert_eq!(adapter, "typesafe-jev"),
        _ => panic!("expected token set"),
    }
}

#[test]
fn mem_decision_token_subcommands_parse_adapter() {
    match parse_mem(&["decision", "token", "clear", "--adapter", "other"]).unwrap() {
        MemAction::Decision {
            action:
                MemDecisionAction::Token {
                    action: MemDecisionTokenAction::Clear { adapter },
                },
        } => assert_eq!(adapter, "other"),
        _ => panic!("expected token clear"),
    }
    match parse_mem(&["decision", "token", "status"]).unwrap() {
        MemAction::Decision {
            action:
                MemDecisionAction::Token {
                    action: MemDecisionTokenAction::Status { adapter },
                },
        } => assert_eq!(adapter, None),
        _ => panic!("expected token status"),
    }
    assert!(matches!(
        parse_mem(&["decision", "status"]).unwrap(),
        MemAction::Decision {
            action: MemDecisionAction::Status
        }
    ));
}

#[test]
fn mem_decision_token_read_refuses_terminal_and_empty_input() {
    let terminal = read_token("secret-token".as_bytes(), true).unwrap_err();
    assert!(!terminal.contains("secret-token"));
    assert!(read_token("".as_bytes(), false).is_err());
    assert!(read_token("  \r\n".as_bytes(), false).is_err());
    assert_eq!(
        read_token("secret-token\r\n".as_bytes(), false).unwrap(),
        "secret-token"
    );
}

#[test]
fn mem_decision_token_error_message_redacts_the_value() {
    let reply = Reply::for_test(400, "decision token store: bad secret-token shape");
    let message = super::decision::error_message(&reply, Some("secret-token"));
    assert!(!message.contains("secret-token"));
    assert!(message.contains("[redacted]"));
}

#[test]
fn mem_decision_render_shows_relevance_and_low_confidence() {
    let value = json!({
        "hits": [
            {"item": {"id": "mem_1", "title": "First", "kind": "fact", "summary": "short"}, "score": 0.5, "relevance": 0.92},
            {"item": {"id": "mem_2", "title": "Second", "kind": "note", "summary": "other"}, "score": 0.25, "relevance": 0.05}
        ],
        "decision_rerank": {"status": "applied", "adapter": "typesafe-jev", "low_confidence": true, "max_relevance": 0.05}
    });

    let out = super::render_search_hits(&value);
    let lines: Vec<&str> = out.lines().collect();

    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], "mem_1 | First | fact | 0.500 | rel 92% | short");
    assert_eq!(lines[1], "mem_2 | Second | note | 0.250 | rel 5% | other");
    assert!(lines[2].contains("nothing relevant"));
    assert!(lines[2].contains("5%"));

    let confident = json!({
        "hits": [{"item": {"id": "a", "title": "T", "kind": "fact", "summary": "s"}, "score": 0.5, "relevance": 0.9}],
        "decision_rerank": {"status": "applied", "low_confidence": false, "max_relevance": 0.9}
    });
    assert_eq!(super::render_search_hits(&confident).lines().count(), 1);
}

#[test]
fn mem_decision_render_reports_fallback_reason() {
    let hit =
        json!({"item": {"id": "a", "title": "T", "kind": "fact", "summary": "s"}, "score": 0.5});
    for (status, reason) in [("fallback", "deadline"), ("skipped", "legacy_rerank")] {
        let value =
            json!({"hits": [hit.clone()], "decision_rerank": {"status": status, "reason": reason}});
        let out = super::render_search_hits(&value);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "a | T | fact | 0.500 | s");
        assert!(lines[1].contains(reason), "{status}: {}", lines[1]);
    }
}

#[test]
fn mem_decision_render_unchanged_without_envelope() {
    let value = json!({"hits": [
        {"item": {"id": "mem_1", "title": "First", "kind": "fact", "summary": "short summary", "body": "ignored"}, "score": 0.8754},
        {"item": {"id": "mem_2", "title": "Second", "kind": "note", "body": "line one\nline two"}, "score": 0.5}
    ]});
    assert_eq!(
        super::render_search_hits(&value),
        "mem_1 | First | fact | 0.875 | short summary\nmem_2 | Second | note | 0.500 | line one line two\n"
    );
    assert_eq!(super::render_search_hits(&json!({"hits": []})), "(empty)\n");
}

#[test]
fn mem_decision_status_render_never_carries_a_token_value() {
    let status = json!({
        "config": {"enabled": false, "adapter": "typesafe-jev", "candidates": 10, "max_concurrency": 8, "deadline_ms": 3000, "low_confidence_threshold": 0.15, "max_body_chars": 8000},
        "adapter_registered": true,
        "adapters": [{
            "id": "typesafe-jev",
            "primitives": {"choice": true, "noul": true, "score": false},
            "token": {"adapter": "typesafe-jev", "present": true, "path": "/data/decision-models/typesafe-jev", "mode_ok": true}
        }]
    });
    let out = super::decision_render::render_status(&status);
    assert!(out.contains("disabled"));
    assert!(out.contains("typesafe-jev | primitives: choice, noul | token present | mode_ok=true"));
    assert!(out.contains("/data/decision-models/typesafe-jev"));

    let absent =
        json!({"adapter": "typesafe-jev", "present": false, "path": "/p", "mode_ok": false});
    assert_eq!(
        super::decision_render::render_token_status(&absent),
        "typesafe-jev: no token stored\n"
    );
}

#[test]
fn mem_decision_token_render_reports_unknown_mode_as_not_applicable() {
    let token = json!({"adapter": "typesafe-jev", "present": true, "path": "/p", "mode_ok": null});
    assert!(super::decision_render::render_token_set(&token).contains("mode_ok: n/a"));
    assert!(super::decision_render::render_token_status(&token).contains("mode_ok=n/a"));
}
