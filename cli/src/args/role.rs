use clap::Subcommand;

#[derive(Subcommand)]
pub enum RoleAction {
    /// Preview a shipped manager's instructions or ask the owner to approve their sync
    #[command(group = clap::ArgGroup::new("sync_mode").required(true).args(["preview", "apply"]))]
    Sync {
        role: String,
        #[arg(long)]
        team: String,
        #[arg(long)]
        preview: bool,
        /// Request the owner's notification choice; this flag never grants approval
        #[arg(long, requires_all = ["current_hash", "replacement_hash"])]
        apply: bool,
        #[arg(long, requires = "apply")]
        current_hash: Option<String>,
        #[arg(long, requires = "apply")]
        replacement_hash: Option<String>,
    },
    /// List exactly the roles a Team has, with kind, Domios authority (a built-in manager's, or none) and package
    List {
        /// Team whose registrations to list; without it (or for a Team with none), the builtin default package
        #[arg(long)]
        team: Option<String>,
    },

    /// Render a role's kind, authority and the exact text it receives at each delivery occasion
    Show {
        /// Role name the Team registers, or watcher
        #[arg(default_value = "worker")]
        role_type: String,
        /// Team whose registration of this type to render
        #[arg(long)]
        team: Option<String>,
        /// Task-owned persona appended after the role's text, as a dispatch supplies it
        #[arg(long, allow_hyphen_values = true)]
        guidance: Option<String>,
        /// Render only this occasion
        #[arg(long)]
        occasion: Option<String>,
    },

    /// Re-read a Team's role registrations from disk
    Reload {
        #[arg(long)]
        team: Option<String>,
    },
}

#[cfg(test)]
#[path = "role/sync_tests.rs"]
mod sync_tests;

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::args::cli::{Cli, Commands};

    use super::RoleAction;

    #[test]
    fn show_parses_a_role_type_team_guidance_and_occasion() {
        let parsed = Cli::try_parse_from([
            "codeterm",
            "role",
            "show",
            "checker",
            "--team",
            "team_a",
            "--guidance",
            "You are Mira.",
            "--occasion",
            "launch_assignment",
        ])
        .expect("role show parses");
        match parsed.command {
            Commands::Role {
                action:
                    RoleAction::Show {
                        role_type,
                        team,
                        guidance,
                        occasion,
                    },
            } => {
                assert_eq!(role_type, "checker");
                assert_eq!(team.as_deref(), Some("team_a"));
                assert_eq!(guidance.as_deref(), Some("You are Mira."));
                assert_eq!(occasion.as_deref(), Some("launch_assignment"));
            }
            _ => panic!("expected role show"),
        }
    }

    #[test]
    fn show_defaults_to_the_worker_type_and_kind_is_gone() {
        let parsed = Cli::try_parse_from(["codeterm", "role", "show"]).expect("role show parses");
        match parsed.command {
            Commands::Role {
                action: RoleAction::Show { role_type, .. },
            } => assert_eq!(role_type, "worker"),
            _ => panic!("expected role show"),
        }
        assert!(Cli::try_parse_from(["codeterm", "role", "show", "--kind", "qa"]).is_err());
    }

    #[test]
    fn list_and_reload_take_an_optional_team_and_persona_is_gone() {
        assert!(matches!(
            Cli::try_parse_from(["codeterm", "role", "list", "--team", "team_a"])
                .expect("role list parses")
                .command,
            Commands::Role {
                action: RoleAction::List { team: Some(_) }
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["codeterm", "role", "reload"])
                .expect("role reload parses")
                .command,
            Commands::Role {
                action: RoleAction::Reload { team: None }
            }
        ));
        assert!(Cli::try_parse_from(["codeterm", "persona", "list"]).is_err());
    }
}
