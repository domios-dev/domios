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
                    },
            } => {
                assert_eq!(orchestrator, "claude:fable");
                assert_eq!(planner, "codex");
                assert_eq!(workspace.as_deref(), Some("ws1"));
                assert_eq!(title.as_deref(), Some("My Plan"));
            }
            _ => panic!("expected team pair"),
        }
    }
}
