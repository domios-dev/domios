use clap::Parser;

use crate::{Cli, Commands, PlanAction};

#[test]
fn tab_name_contract_cli_accepts_executor_name_and_keeps_role_slug_distinct() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "task",
        "add",
        "pl_1",
        "--name",
        "Stable task",
        "--executor-role",
        "rust-backend-dev",
        "--personality",
        "Ada",
        "--role-guidance",
        "Own the backend change end to end.",
    ])
    .expect("CLI accepts a one-word executor name and role slug");

    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action:
                        crate::args::plan::PlanTaskAction::Add {
                            executor_role,
                            personality,
                            ..
                        },
                },
        } => {
            assert_eq!(executor_role, "rust-backend-dev");
            assert_eq!(personality.as_deref(), Some("Ada"));
            assert_eq!(
                codeterm_types::plan::validate_executor_name(personality.as_deref().unwrap())
                    .unwrap(),
                "Ada"
            );
        }
        _ => panic!("expected plan task add"),
    }
}

#[test]
fn tab_name_contract_cli_rejects_role_slug_and_multi_word_executor_names() {
    for value in ["rust-backend-dev", "Ada Lovelace"] {
        assert!(
            codeterm_types::plan::validate_executor_name(value).is_err(),
            "{value}"
        );
    }
}
