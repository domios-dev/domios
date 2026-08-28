use clap::Parser;

use crate::Cli;

#[test]
fn diagnostics_that_speak_cross_subcommand_execution_intent_names_both_commands() {
    let error = Cli::try_parse_from([
        "codeterm", "plan", "task", "add", "pl_1", "--name", "task", "--executor-role", "worker",
        "--execution-intent", "read_only",
    ])
    .expect_err("dispatch intent must not parse for task add")
    .to_string();
    assert!(error.contains("plan task add"), "{error}");
    assert!(error.contains("plan task dispatch"), "{error}");
}
