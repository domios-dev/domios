use clap::Parser;

use crate::Cli;

#[test]
fn diagnostics_that_speak_cross_subcommand_execution_intent_names_both_commands() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "task",
        "add",
        "pl_1",
        "--name",
        "task",
        "--executor-role",
        "worker",
        "--execution-intent",
        "read_only",
    ]);
    let error = match parsed {
        Ok(_) => panic!("dispatch intent must not parse for task add"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("plan task add"), "{error}");
    assert!(error.contains("plan task dispatch"), "{error}");
}
