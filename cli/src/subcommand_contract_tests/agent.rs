//! `agent spawn` CLI contract: help text and flag dependencies must match
//! behaviour.

use clap::{CommandFactory, Parser};

use crate::args::agent::AgentAction;

#[derive(Parser)]
struct AgentOnly {
    #[command(subcommand)]
    action: AgentAction,
}

#[test]
fn provider_switch_parses_target_selection_and_requires_a_provider() {
    let parsed = AgentOnly::try_parse_from([
        "codeterm",
        "switch-provider",
        "target-token",
        "--tab",
        "tab-token",
        "--profile",
        "profile-token",
        "--model",
        "model-token",
        "--reasoning-effort",
        "effort-token",
        "--force",
    ])
    .unwrap();
    assert!(
        matches!(parsed.action, AgentAction::SwitchProvider { provider, tab: Some(tab), launch_profile: Some(profile), model: Some(model), reasoning_effort: Some(effort), force: true } if provider == "target-token" && tab == "tab-token" && profile == "profile-token" && model == "model-token" && effort == "effort-token")
    );
    let without_profile = AgentOnly::try_parse_from([
        "codeterm",
        "switch-provider",
        "target-token",
        "--tab",
        "tab-token",
    ])
    .unwrap();
    assert!(matches!(
        without_profile.action,
        AgentAction::SwitchProvider {
            launch_profile: None,
            force: false,
            ..
        }
    ));
    assert!(
        AgentOnly::try_parse_from(["codeterm", "switch-provider", "--tab", "tab-token"]).is_err()
    );
}

#[test]
fn provider_switch_mesh_api_map_forwards_launch_profile() {
    let parsed = crate::Cli::try_parse_from([
        "domios",
        "agent",
        "switch-provider",
        "target-token",
        "--tab",
        "tab-token",
        "--profile",
        "profile-token",
        "--model",
        "model-token",
        "--reasoning-effort",
        "effort-token",
        "--force",
    ])
    .unwrap();
    let (method, path, body) = crate::mesh_cli::api_map::cli_command_to_api(&parsed.command);
    assert_eq!(method, "POST");
    assert_eq!(path, "/api/tabs/tab-token/agent/switch-provider");
    let body: serde_json::Value = serde_json::from_str(&body.unwrap()).unwrap();
    assert_eq!(body["provider"], "target-token");
    assert_eq!(body["launch_profile"], "profile-token");
    assert_eq!(body["model"], "model-token");
    assert_eq!(body["reasoning_effort"], "effort-token");
    assert_eq!(body["force"], true);

    let parsed_without_profile = crate::Cli::try_parse_from([
        "domios",
        "agent",
        "switch-provider",
        "target-token",
        "--tab",
        "tab-token",
    ])
    .unwrap();
    let (method, path, body) =
        crate::mesh_cli::api_map::cli_command_to_api(&parsed_without_profile.command);
    assert_eq!(method, "POST");
    assert_eq!(path, "/api/tabs/tab-token/agent/switch-provider");
    let body: serde_json::Value = serde_json::from_str(&body.unwrap()).unwrap();
    assert_eq!(body["provider"], "target-token");
    assert!(body.get("launch_profile").is_none());
}

#[test]
fn provider_switch_status_reads_a_specific_daemon_operation() {
    let parsed = AgentOnly::try_parse_from([
        "codeterm",
        "switch-provider-status",
        "operation-token",
        "--tab",
        "main",
    ])
    .unwrap();
    assert!(
        matches!(parsed.action, AgentAction::SwitchProviderStatus { operation, tab: Some(tab) } if operation == "operation-token" && tab == "main")
    );
    assert!(AgentOnly::try_parse_from(["codeterm", "switch-provider-status"]).is_err());
}

#[test]
fn fresh_restart_parses_and_rejects_provider_handoff() {
    let parsed = AgentOnly::try_parse_from([
        "codeterm", "restart", "--tab", "tab-id", "--fresh", "--force",
    ])
    .unwrap();
    assert!(
        matches!(parsed.action, AgentAction::Restart { fresh: true, force: true, tab: Some(tab), .. } if tab == "tab-id")
    );
    assert!(AgentOnly::try_parse_from([
        "codeterm",
        "restart",
        "--fresh",
        "--provider",
        "fixture-provider"
    ])
    .is_err());
    assert!(
        AgentOnly::try_parse_from(["codeterm", "restart", "--fresh", "--handoff-history"]).is_err()
    );
}

/// Pull help from the Clap definition itself — mechanism, not shelled prose.
#[test]
fn parent_tab_help_names_planners() {
    let spawn = AgentOnly::command()
        .find_subcommand("spawn")
        .expect("`agent spawn` subcommand must exist")
        .clone();
    let parent_tab_arg = spawn
        .get_arguments()
        .find(|a| a.get_long() == Some("parent-tab"))
        .expect("`agent spawn` must expose --parent-tab");
    let help = parent_tab_arg
        .get_help()
        .map(|h| h.to_string())
        .unwrap_or_default();

    assert_ne!(
        help.trim(),
        "Parent tab ID (for Workers)",
        "the stale Worker-only help text must be replaced: {help}"
    );
    assert!(
        help.contains("Planner"),
        "--parent-tab help must name Planners: {help}"
    );
    assert!(
        help.to_lowercase().contains("plan"),
        "--parent-tab help must say a Planner's parent resolves from --plan: {help}"
    );
}

fn spawn_command() -> clap::Command {
    AgentOnly::command()
        .find_subcommand("spawn")
        .expect("`agent spawn` subcommand must exist")
        .clone()
}

/// The defect this closes: `--execution-intent` was declared as requiring
/// `--plan-task`, so the read-only verification Worker the operations doc
/// prescribes could not be spawned at all. Asserted by parsing the exact shape
/// that was refused, never against the wording of any refusal.
#[test]
fn execution_intent_parses_without_a_plan_task() {
    AgentOnly::try_parse_from([
        "agent",
        "spawn",
        "codex",
        "--role",
        "worker",
        "--execution-intent",
        "read_only",
    ])
    .expect("--execution-intent must not require --plan-task");
}

/// The verifier is command-scoped, so the flag that scopes it is spelled the
/// same on both paths that can declare gates.
#[test]
fn spawn_carries_the_same_authorized_gate_flag_as_dispatch() {
    let spawn = spawn_command();
    let gate = spawn
        .get_arguments()
        .find(|arg| arg.get_long() == Some("authorized-gate"))
        .expect("`agent spawn` must expose --authorized-gate");
    assert!(
        matches!(gate.get_action(), clap::ArgAction::Append),
        "--authorized-gate must be repeatable"
    );
}
