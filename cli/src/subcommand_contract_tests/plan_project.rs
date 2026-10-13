#[test]
fn plan_set_project_preserves_positional_target_host_path() {
    use clap::Parser;
    let command = crate::Cli::try_parse_from([
        "codeterm",
        "plan",
        "set-project",
        "pl-existing",
        "C:/Developer/codeterm",
    ])
    .unwrap()
    .command;
    assert!(matches!(command, crate::Commands::Plan {
        action: crate::PlanAction::SetProject { plan_id, path }
    } if plan_id == "pl-existing" && path == "C:/Developer/codeterm"));
}
