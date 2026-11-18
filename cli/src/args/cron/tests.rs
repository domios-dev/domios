use clap::Parser;

#[test]
fn cron_create_requires_a_meaningful_name() {
    let valid = crate::args::cli::Cli::try_parse_from([
        "codeterm",
        "cron",
        "create",
        "build-result-notify",
        "--schedule",
        "0 * * * *",
        "--task",
        "check build",
    ]);
    assert!(valid.is_ok());
    for name in ["ab", "cron_5d22889a8898"] {
        assert!(crate::args::cli::Cli::try_parse_from([
            "codeterm",
            "cron",
            "create",
            name,
            "--schedule",
            "0 * * * *",
            "--task",
            "check build",
        ])
        .is_err());
    }
    assert!(crate::args::cli::Cli::try_parse_from([
        "codeterm",
        "cron",
        "create",
        "--schedule",
        "0 * * * *",
        "--task",
        "check build",
    ])
    .is_err());
}

#[test]
fn cron_edit_target_patch_distinguishes_set_clear_and_unchanged() {
    for (flags, expected) in [
        (vec![], None),
        (vec!["--tab", "new-owner"], Some("new-owner")),
        (vec!["--clear-tab"], Some("")),
    ] {
        let mut args = vec!["domios", "cron", "edit", "stable-job"];
        args.extend(flags);
        let cli = crate::args::cli::Cli::try_parse_from(args).unwrap();
        let crate::Commands::Cron { action } = cli.command else {
            panic!("expected cron");
        };
        let (id, body) = action.update_request().unwrap();
        assert_eq!(id, "stable-job");
        assert_eq!(
            body.get("report_target_tab_id")
                .and_then(|value| value.as_str()),
            expected
        );
        let (method, path, encoded) =
            crate::mesh_cli::api_map::cli_command_to_api(&crate::Commands::Cron { action });
        assert_eq!(method, "PUT");
        assert_eq!(path, "/api/crons/stable-job");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&encoded.unwrap()).unwrap(),
            body
        );
    }
    assert!(crate::args::cli::Cli::try_parse_from([
        "domios",
        "cron",
        "edit",
        "stable-job",
        "--tab",
        "new-owner",
        "--clear-tab",
    ])
    .is_err());
}
