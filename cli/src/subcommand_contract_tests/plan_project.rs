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

#[test]
fn plan_create_accepts_from_spec_and_refuses_text_shortcuts() {
    use clap::Parser;

    for flag in ["--spec", "--spec-file"] {
        let error = crate::Cli::try_parse_from([
            "domios",
            "plan",
            "create",
            "--title",
            "Opaque",
            flag,
            "old-input",
        ])
        .err()
        .expect("retired flag must fail during parsing");
        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
    }
    let parsed = crate::Cli::try_parse_from([
        "domios",
        "plan",
        "create",
        "--title",
        "Opaque",
        "--from-spec",
        "spec_current",
    ])
    .unwrap();
    assert!(matches!(parsed.command, crate::Commands::Plan {
        action: crate::PlanAction::Create { from_spec: Some(id), .. }
    } if id == "spec_current"));
}
