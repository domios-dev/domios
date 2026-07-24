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
}
