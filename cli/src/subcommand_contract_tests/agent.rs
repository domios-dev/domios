//! `agent spawn` CLI contract: `--parent-tab` help text must match behaviour.

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
