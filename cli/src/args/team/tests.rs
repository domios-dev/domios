use clap::Parser;

use crate::{Cli, Commands};

use super::TeamAction;

#[test]
fn team_verbs_parse() {
    let parsed =
        Cli::try_parse_from(["codeterm", "team", "adopt-plan", "team_alpha", "pl_one"]).unwrap();
    assert!(matches!(
        parsed.command,
        Commands::Team {
            action: TeamAction::AdoptPlan { team_id, plan_id }
        } if team_id == "team_alpha" && plan_id == "pl_one"
    ));
}

#[test]
fn adopt_agent_parses_team_and_tab_positionals() {
    let parsed =
        Cli::try_parse_from(["codeterm", "team", "adopt-agent", "team_alpha", "tab_seven"])
            .expect("team adopt-agent parses");
    assert!(matches!(
        parsed.command,
        Commands::Team {
            action: TeamAction::AdoptAgent { team_id, tab_id, peer: None }
        } if team_id == "team_alpha" && tab_id == "tab_seven"
    ));
}

#[test]
fn adopt_agent_requires_both_positionals() {
    assert!(Cli::try_parse_from(["codeterm", "team", "adopt-agent", "team_alpha"]).is_err());
}

#[test]
fn attach_cartographer_parses_provider_and_model() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "team",
        "attach-cartographer",
        "team_alpha",
        "--provider",
        "provider-neutral",
        "--model",
        "model-1",
        "--brief-file",
        "brief.json",
    ])
    .expect("team cartographer attachment parses");
    assert!(matches!(
        parsed.command,
        Commands::Team {
            action: TeamAction::AttachCartographer {
                team_id,
                provider,
                model,
                brief_file,
            }
        } if team_id == "team_alpha" && provider == "provider-neutral" && model.as_deref() == Some("model-1") && brief_file.as_deref() == Some("brief.json")
    ));
}

#[test]
fn pair_parses_provider_model_specs_and_optional_fields() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "team",
        "pair",
        "--orchestrator",
        "claude:fable",
        "--planner",
        "codex",
        "--workspace",
        "ws1",
        "--title",
        "My Plan",
    ])
    .expect("team pair parses");
    match parsed.command {
        Commands::Team {
            action:
                TeamAction::Pair {
                    orchestrator,
                    planner,
                    workspace,
                    title,
                    acknowledge_leaf,
                },
        } => {
            assert_eq!(orchestrator, "claude:fable");
            assert_eq!(planner, "codex");
            assert_eq!(workspace.as_deref(), Some("ws1"));
            assert_eq!(title.as_deref(), Some("My Plan"));
            assert!(!acknowledge_leaf, "the flag defaults to false when absent");
        }
        _ => panic!("expected team pair"),
    }
}

/// codeterm#400: a leaf caller declares intent with `--acknowledge-leaf`,
/// carried through to the daemon so a leaf-attributed pair is never
/// created silently.
#[test]
fn pair_parses_the_acknowledge_leaf_declaration() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "team",
        "pair",
        "--orchestrator",
        "claude",
        "--planner",
        "codex",
        "--acknowledge-leaf",
    ])
    .expect("team pair --acknowledge-leaf parses");
    match parsed.command {
        Commands::Team {
            action: TeamAction::Pair {
                acknowledge_leaf, ..
            },
        } => assert!(acknowledge_leaf),
        _ => panic!("expected team pair"),
    }
}
