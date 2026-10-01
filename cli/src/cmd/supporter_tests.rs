use super::{error_message, read_token, Reply};
use crate::args::supporter::{SupporterAction, SupporterTokenAction};
use crate::cmd::supporter_render::{render_models, render_status, render_token_status};
use crate::{Cli, Commands, MemAction, SupporterMode};
use clap::Parser;
use serde_json::json;

fn parse(argv: &[&str]) -> Result<Commands, clap::Error> {
    let mut full = vec!["codeterm"];
    full.extend_from_slice(argv);
    Cli::try_parse_from(full).map(|cli| cli.command)
}

fn parse_mem(argv: &[&str]) -> Result<MemAction, clap::Error> {
    match parse(argv)? {
        Commands::Mem { action } => Ok(action),
        _ => panic!("expected mem command"),
    }
}

#[test]
fn supporter_parse_enable_disable_status_use_features() {
    assert!(matches!(parse(&["supporter", "enable"]).unwrap(), Commands::Supporter { action: SupporterAction::Enable }));
    assert!(matches!(parse(&["supporter", "disable"]).unwrap(), Commands::Supporter { action: SupporterAction::Disable }));
    assert!(matches!(parse(&["supporter", "status"]).unwrap(), Commands::Supporter { action: SupporterAction::Status }));
    assert!(matches!(parse(&["supporter", "models"]).unwrap(), Commands::Supporter { action: SupporterAction::Models }));
    assert!(matches!(
        parse(&["supporter", "use", "fixture", "--model", "listed"]).unwrap(),
        Commands::Supporter { action: SupporterAction::Use { adapter, model } }
            if adapter == "fixture" && model.as_deref() == Some("listed")
    ));
    assert!(matches!(
        parse(&["supporter", "features", "--memory-rerank", "on", "--tab-state", "off"]).unwrap(),
        Commands::Supporter { action: SupporterAction::Features { memory_rerank: Some(SupporterMode::On), tab_state: Some(SupporterMode::Off) } }
    ));
}

#[test]
fn supporter_token_set_has_no_value_flag() {
    assert!(matches!(
        parse(&["supporter", "token", "set"]).unwrap(),
        Commands::Supporter { action: SupporterAction::Token { action: SupporterTokenAction::Set { adapter: None } } }
    ));
    for flag in ["--token", "--value", "--password"] {
        assert!(parse(&["supporter", "token", "set", flag, "secret"]).is_err(), "{flag} must be rejected");
    }
}

#[test]
fn supporter_token_set_refuses_tty() {
    let terminal = read_token("secret".as_bytes(), true).unwrap_err();
    assert!(!terminal.contains("secret"));
    assert!(read_token("".as_bytes(), false).is_err());
    assert!(read_token("  \r\n".as_bytes(), false).is_err());
    assert_eq!(read_token("secret\r\n".as_bytes(), false).unwrap(), "secret");
}

#[test]
fn supporter_use_unknown_lists_registered() {
    let reply = Reply::for_test(
        404,
        r#"{"error":{"message":"supporter adapter is not registered: absent; registered adapters: fixture-a, fixture-b"}}"#,
    );
    let message = error_message(&reply, None);
    assert!(message.contains("fixture-a, fixture-b"));
    assert!(message.contains("absent"));
}

#[test]
fn mem_search_supporter_flag_on_off() {
    for (value, expected) in [("on", true), ("off", false)] {
        assert!(matches!(
            parse_mem(&["search", "q", "--supporter", value]).unwrap(),
            MemAction::Search { supporter: Some(mode), .. } if mode.enabled() == expected
        ));
    }
    assert!(matches!(parse_mem(&["search", "q"]).unwrap(), MemAction::Search { supporter: None, .. }));
}

#[test]
fn old_mem_decision_verb_is_rejected() {
    assert!(parse(&["mem", "decision", "status"]).is_err());
}

#[test]
fn old_decision_rerank_flag_is_rejected() {
    let removed_flag = ["--decision", "-rerank"].concat();
    assert!(parse_mem(&["search", "q", &removed_flag, "on"]).is_err());
}

#[test]
fn supporter_models_and_status_render_discovered_state() {
    let status = json!({
        "state": "active", "adapter": "fixture", "model": "model-a", "adapter_registered": true,
        "deadline_ms": 3000, "features": {"memory_rerank": true, "tab_state": false},
        "token": {"adapter": "fixture", "present": true, "mode_ok": true},
        "adapters": [{"id": "fixture", "primitives": {"choice": true}, "models": [{"id":"model-a", "display_name":"Model A"}], "token":{"present":true,"mode_ok":true}}]
    });
    let text = render_status(&status);
    assert!(text.contains("Active"));
    assert!(text.contains("Adapter: fixture"));
    assert!(text.contains("Model: model-a"));
    assert!(text.contains("Memory re-rank: on"));
    assert!(text.contains("Tab-state: off"));
    assert!(!text.contains("secret"));
    let models = render_models(&json!({"adapters":[{"id":"fixture","models":[{"id":"model-a","display_name":"Model A"}]}]}));
    assert!(models.contains("Model A (model-a)"));
    assert_eq!(render_token_status(&json!({"adapter":"fixture","present":false})), "fixture: No token\n");
}
