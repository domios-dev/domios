use clap::Subcommand;

#[derive(Subcommand)]
pub enum TeamAction {
    /// List registered Teams
    List,
    /// Show one Team
    Show {
        /// Team ID
        team_id: String,
    },
    /// Give a Team a human-readable name
    Rename {
        /// Team ID
        team_id: String,
        /// New Team name
        name: String,
    },
    /// Close every member tab and remove the Team registry entry
    Close {
        /// Team ID
        team_id: String,
    },
    /// Bind an existing plan to a Team
    AdoptPlan {
        /// Team ID
        team_id: String,
        /// Plan ID
        plan_id: String,
    },
    /// Attach an already-running roleless agent tab to a Team
    AdoptAgent {
        /// Team ID
        team_id: String,
        /// Tab ID of the running agent
        tab_id: String,
    },
    /// Create an Orchestrator + Planner Team in one action. The Plan stays
    /// unbound until the first goal.
    Pair {
        /// Orchestrator provider, optionally with a model (`provider[:model]`)
        #[arg(long)]
        orchestrator: String,
        /// Planner provider, optionally with a model (`provider[:model]`)
        #[arg(long)]
        planner: String,
        #[arg(long, env = "CODETERM_WORKSPACE_ID")]
        workspace: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        title: Option<String>,
        /// Explicit "I know I am a leaf and I am doing this deliberately"
        /// declaration — required only when the calling tab holds a Manager
        /// or Worker role. Creation is then attributed to this tab, never to
        /// an intermediary shell.
        #[arg(long)]
        acknowledge_leaf: bool,
    },
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::{Cli, Commands};

    use super::TeamAction;

    #[test]
    fn team_verbs_parse() {
        let parsed =
            Cli::try_parse_from(["codeterm", "team", "adopt-plan", "team_alpha", "pl_one"])
                .unwrap();
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
                action: TeamAction::AdoptAgent { team_id, tab_id }
            } if team_id == "team_alpha" && tab_id == "tab_seven"
        ));
    }

    #[test]
    fn adopt_agent_requires_both_positionals() {
        assert!(Cli::try_parse_from(["codeterm", "team", "adopt-agent", "team_alpha"]).is_err());
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
                action:
                    TeamAction::Pair {
                        acknowledge_leaf, ..
                    },
            } => assert!(acknowledge_leaf),
            _ => panic!("expected team pair"),
        }
    }
}
