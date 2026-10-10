use super::{error_message, read_token, Reply};
use crate::args::supporter::{SupporterAction, SupporterTokenAction};
use crate::cmd::supporter_render::{render_models, render_status, render_token_status};
use crate::{Cli, Commands, MemAction};
use clap::{CommandFactory, Parser};
use serde_json::json;

fn parse(argv: &[&str]) -> Result<Commands, clap::Error> {
    let mut full = vec!["codeterm"];
    full.extend_from_slice(argv);
    Cli::try_parse_from(full).map(|cli| cli.command)
}

fn parse_mem(argv: &[&str]) -> Result<MemAction, clap::Error> {
    let mut full = vec!["mem"];
    full.extend_from_slice(argv);
    match parse(&full)? {
        Commands::Mem { action } => Ok(action),
        _ => panic!("expected mem command"),
    }
}

#[test]
fn spinal_cord_alias_uses_supporter_command_tree() {
    for name in ["supporter", "spinal-cord"] {
        assert!(matches!(
            parse(&[name, "status"]).unwrap(),
            Commands::Supporter {
                action: SupporterAction::Status
            }
        ));
        assert!(matches!(
            parse(&[name, "use", "fixture", "--model", "listed"]).unwrap(),
            Commands::Supporter { action: SupporterAction::Use { adapter, model } }
                if adapter == "fixture" && model.as_deref() == Some("listed")
        ));
        assert!(matches!(
            parse(&[name, "token", "set", "--adapter", "fixture"]).unwrap(),
            Commands::Supporter { action: SupporterAction::Token {
                action: SupporterTokenAction::Set { adapter: Some(adapter) }
            } } if adapter == "fixture"
        ));
        assert!(parse(&[name, "token", "set", "secret"]).is_err());
    }
    let command = Cli::command();
    assert!(command
        .get_subcommands()
        .all(|child| child.get_name() != "spinal-cord"));
    let supporter = command.find_subcommand("supporter").unwrap();
    assert!(supporter
        .get_visible_aliases()
        .any(|alias| alias == "spinal-cord"));
}

#[test]
fn supporter_help_names_spinal_cord() {
    for name in ["supporter", "spinal-cord"] {
        let help = Cli::try_parse_from(["codeterm", name, "--help"])
            .err()
            .unwrap();
        assert_eq!(help.kind(), clap::error::ErrorKind::DisplayHelp);
        assert!(help.to_string().contains("Spinal Cord (Supporter Agent)"));
    }
}

#[test]
fn supporter_enable_disable_features_rejected() {
    for removed in ["enable", "disable", "features"] {
        assert!(parse(&["supporter", removed]).is_err());
    }
    assert!(matches!(
        parse(&["supporter", "status"]).unwrap(),
        Commands::Supporter {
            action: SupporterAction::Status
        }
    ));
    assert!(matches!(
        parse(&["supporter", "models"]).unwrap(),
        Commands::Supporter {
            action: SupporterAction::Models
        }
    ));
    assert!(matches!(
        parse(&["supporter", "use", "fixture", "--model", "listed"]).unwrap(),
        Commands::Supporter { action: SupporterAction::Use { adapter, model } }
            if adapter == "fixture" && model.as_deref() == Some("listed")
    ));
}

#[test]
fn supporter_token_set_has_no_value_flag() {
    assert!(matches!(
        parse(&["supporter", "token", "set"]).unwrap(),
        Commands::Supporter {
            action: SupporterAction::Token {
                action: SupporterTokenAction::Set { adapter: None }
            }
        }
    ));
    for flag in ["--token", "--value", "--password"] {
        assert!(
            parse(&["supporter", "token", "set", flag, "secret"]).is_err(),
            "{flag} must be rejected"
        );
    }
}

#[test]
fn supporter_token_set_refuses_tty() {
    let terminal = read_token("secret".as_bytes(), true).unwrap_err();
    assert!(!terminal.contains("secret"));
    assert!(read_token("".as_bytes(), false).is_err());
    assert!(read_token("  \r\n".as_bytes(), false).is_err());
    assert_eq!(
        read_token("secret\r\n".as_bytes(), false).unwrap(),
        "secret"
    );
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
    assert!(matches!(
        parse_mem(&["search", "q"]).unwrap(),
        MemAction::Search {
            supporter: None,
            ..
        }
    ));
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
        "deadline_ms": 3000, "calls_last_hour": 12, "calls_last_day": 45, "last_activity": 123, "budget": {"reached": true, "calls_per_hour": 600, "free_at": 3600},
        "token": {"adapter": "fixture", "present": true, "mode_ok": true},
        "adapters": [{
            "id": "fixture",
            "primitives": {"choice": true},
            "models": [{"id":"model-a", "display_name":"Model A"}],
            "token":{"present":true,"mode_ok":true}
        }]
    });
    let text = render_status(&status);
    assert!(text.contains("Active"));
    assert!(text.contains("Adapter: fixture"));
    assert!(text.contains("Model: model-a"));
    assert!(text.contains("Calls: 12 in the last hour / 45 in the last day"));
    assert!(text.contains("Budget: reached (cap 600 / hour, free at 3600)"));
    assert!(!text.contains("secret"));
    let models = render_models(&json!({"adapters":[{
        "id":"fixture",
        "models":[{"id":"model-a","display_name":"Model A"}]
    }]}));
    assert!(models.contains("Model A (model-a)"));
    assert_eq!(
        render_token_status(&json!({"adapter":"fixture","present":false})),
        "fixture: No token\n"
    );
}

#[test]
fn supporter_ready_status_renders_optional_token_mode_without_a_token() {
    let status = json!({
        "state": "active", "token_mode": "optional", "adapter": "fixture", "model": "local-model",
        "adapter_registered": true, "deadline_ms": 3000,
        "token": {"adapter": "fixture", "token_mode": "optional", "present": false, "mode_ok": null},
        "adapters": []
    });
    let text = render_status(&status);
    assert!(text.contains("Active"));
    assert!(text.contains("Token mode: optional"));
    assert!(text.contains("No token"));
}

#[test]
fn supporter_status_renders_usage_and_credit_exhaustion() {
    let text = render_status(&json!({ "state": "credit_exhausted", "spend": {
        "today": { "input_tokens": 123, "output_tokens": 45, "estimated_usd": 0.042 },
        "month": { "input_tokens": 246, "output_tokens": 90, "estimated_usd": 0.084 },
        "exhausted": true, "next_probe_at": 1700003600
    }}));
    assert!(text.contains("Credit exhausted"));
    assert!(text.contains("123 input / 45 output tokens"));
    assert!(text.contains("estimated USD 0.042"));
    assert!(text.contains("next request probe after 1700003600"));
}

#[test]
fn supporter_status_optional_request_cap_is_positive_only() {
    use crate::cmd::supporter_render::request_cap;
    for cap in [serde_json::Value::Null, json!(0)] {
        assert_eq!(
            request_cap(&json!({ "budget": { "calls_per_hour": cap } })),
            None
        );
    }
    assert_eq!(
        request_cap(&json!({ "budget": { "calls_per_hour": 120 } })),
        Some(120)
    );
}

#[test]
fn supporter_disconnect_contract_parses_adapter_and_use_remains_connect_or_activate() {
    let disconnect = Cli::try_parse_from(["codeterm", "supporter", "disconnect", "fixture"]);
    assert!(disconnect.is_ok());
    let activate =
        Cli::try_parse_from(["codeterm", "supporter", "use", "fixture", "--model", "one"]);
    assert!(activate.is_ok());
}

#[test]
fn supporter_unknown_token_status_is_never_no_token() {
    let text = render_token_status(&json!({ "adapter": "fixture" }));
    assert!(text.contains("Token status unavailable"));
    assert!(!text.contains("No token"));
}
