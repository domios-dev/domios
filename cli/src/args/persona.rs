use clap::Subcommand;

#[derive(Subcommand)]
pub enum PersonaAction {
    /// List every role kind the resolver can reach, with refused overlays
    List,

    /// Render the exact preset text a role kind receives at each delivery occasion
    Show {
        /// Structural role — Manager or Worker
        #[arg(long, default_value = "Worker")]
        role: String,
        /// Role kind id (`orchestrator`, `planner`, a Worker subtype, ...)
        #[arg(long)]
        kind: Option<String>,
        /// Task-owned persona layered over the kind's base, as a dispatch supplies it
        #[arg(long, allow_hyphen_values = true)]
        guidance: Option<String>,
        /// Render only this occasion
        #[arg(long)]
        occasion: Option<String>,
    },

    /// Re-read the overlay directory without restarting or rebuilding
    Reload,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::args::cli::{Cli, Commands};

    use super::PersonaAction;

    #[test]
    fn show_parses_role_kind_guidance_and_occasion() {
        let parsed = Cli::try_parse_from([
            "codeterm",
            "persona",
            "show",
            "--role",
            "Worker",
            "--kind",
            "rust-backend-dev",
            "--guidance",
            "You are Mira.",
            "--occasion",
            "launch_assignment",
        ])
        .expect("persona show parses");
        match parsed.command {
            Commands::Persona {
                action:
                    PersonaAction::Show {
                        role,
                        kind,
                        guidance,
                        occasion,
                    },
            } => {
                assert_eq!(role, "Worker");
                assert_eq!(kind.as_deref(), Some("rust-backend-dev"));
                assert_eq!(guidance.as_deref(), Some("You are Mira."));
                assert_eq!(occasion.as_deref(), Some("launch_assignment"));
            }
            _ => panic!("expected persona show"),
        }
    }

    #[test]
    fn show_defaults_to_the_worker_role_and_no_kind() {
        let parsed =
            Cli::try_parse_from(["codeterm", "persona", "show"]).expect("persona show parses");
        match parsed.command {
            Commands::Persona {
                action: PersonaAction::Show { role, kind, .. },
            } => {
                assert_eq!(role, "Worker");
                assert!(kind.is_none());
            }
            _ => panic!("expected persona show"),
        }
    }

    #[test]
    fn list_and_reload_parse() {
        assert!(matches!(
            Cli::try_parse_from(["codeterm", "persona", "list"])
                .expect("persona list parses")
                .command,
            Commands::Persona {
                action: PersonaAction::List
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["codeterm", "persona", "reload"])
                .expect("persona reload parses")
                .command,
            Commands::Persona {
                action: PersonaAction::Reload
            }
        ));
    }
}
