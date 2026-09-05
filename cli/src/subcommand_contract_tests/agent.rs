//! `agent spawn` CLI contract: help text and flag dependencies must match
//! behaviour.

use clap::{CommandFactory, Parser};

use crate::args::agent::AgentAction;

#[derive(Parser)]
struct AgentOnly {
    #[command(subcommand)]
    action: AgentAction,
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
        "Worker",
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
