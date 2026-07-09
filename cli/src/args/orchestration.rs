use clap::Subcommand;

#[derive(Subcommand)]
pub enum OrchestrationAction {
    /// Create an Orchestrator + Planner pair bound to a fresh plan in one action
    New {
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

    use crate::args::cli::{Cli, Commands};

    use super::OrchestrationAction;

    #[test]
    fn new_parses_provider_model_specs_and_optional_fields() {
        let parsed = Cli::try_parse_from([
            "codeterm",
            "orchestration",
            "new",
            "--orchestrator",
            "claude:fable",
            "--planner",
            "codex",
            "--workspace",
            "ws1",
            "--title",
            "My Plan",
        ])
        .expect("orchestration new parses");
        match parsed.command {
            Commands::Orchestration {
                action:
                    OrchestrationAction::New {
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
            _ => panic!("expected orchestration new"),
        }
    }
}
