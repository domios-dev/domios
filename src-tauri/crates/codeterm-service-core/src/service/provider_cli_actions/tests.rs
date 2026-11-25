use super::*;
#[test]
fn a_long_install_hint_still_fits_the_tab_description() {
    let long = format!(
        "Install CLI for fixture; {}",
        "needs a runner first. ".repeat(20)
    );
    let fitted = fit_tab_description(&long);
    assert!(super::super::normalize_tab_description(&fitted).is_ok());
    assert_eq!(
        fitted.chars().count(),
        super::super::MAX_TAB_DESCRIPTION_CHARS
    );
    assert_eq!(
        fit_tab_description("Install CLI for fixture"),
        "Install CLI for fixture"
    );
    assert_eq!(fit_tab_description("two\nlines"), "two lines");
}
#[test]
fn completion_routes_shells_to_records_and_agents_to_reports() {
    assert_eq!(completion_route(None, false), CompletionRoute::SystemLog);
    assert_eq!(
        completion_route(Some("fixture"), false),
        CompletionRoute::AgentReport
    );
    assert_eq!(completion_route(None, true), CompletionRoute::AgentReport);
}
#[test]
fn failed_install_completion_keeps_missing_prerequisite_guidance() {
    let mut status = codeterm_plugin::provider_cli::no_cli();
    status.required = true;
    status.installed = Some(false);
    status.installer_prerequisites = vec![codeterm_plugin::provider_cli::CommandPrerequisite {
        binary: "fixture-runner".into(),
        installed: Some(false),
        hint: "opaque setup guidance".into(),
    }];
    let summary = completion_summary(
        "fixture",
        CliOperation::Install,
        Some(ExecCompletion::CommandFinished {
            exit_code: Some(127),
        }),
        Some(&status),
    );
    assert!(summary.contains("fixture-runner"));
    assert!(summary.contains("opaque setup guidance"));
    assert!(summary.contains("exit code 127"));
}
#[test]
fn no_cli_or_unsupported_os_refuses_before_launch() {
    let mut status = codeterm_plugin::provider_cli::no_cli();
    assert!(operation_command(&status, CliOperation::Install, "linux").is_err());
    status.required = true;
    status.installer = Some("opaque installer".into());
    assert_eq!(
        operation_command(&status, CliOperation::Install, "linux").unwrap(),
        "opaque installer"
    );
    assert!(operation_command(&status, CliOperation::Update, "linux").is_err());
    status.updater = Some("opaque updater".into());
    assert_eq!(
        operation_command(&status, CliOperation::Update, "linux").unwrap(),
        "opaque updater"
    );
}
#[test]
fn result_never_confuses_dispatch_timeout_or_failure_with_success() {
    let mut status = codeterm_plugin::provider_cli::no_cli();
    status.required = true;
    status.installed = Some(false);
    assert!(completion_summary(
        "fixture",
        CliOperation::Install,
        Some(ExecCompletion::CommandFinished { exit_code: Some(0) }),
        Some(&status)
    )
    .contains("still missing"));
    assert!(completion_summary(
        "fixture",
        CliOperation::Update,
        Some(ExecCompletion::CommandFinished { exit_code: Some(7) }),
        Some(&status)
    )
    .contains("exit code 7"));
    assert!(completion_summary("fixture", CliOperation::Install, None, None).contains("timed out"));
}

#[test]
fn explicit_update_timeout_binds_only_to_a_live_agent_owner() {
    let (mut mux, owner) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.snapshot().workspaces[0].id.clone();
    let shell_tab = mux.create_tab(&workspace_id).unwrap();
    mux.get_tab_mut(&owner).unwrap().agent_type = Some("fixture-provider".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);

    assert_eq!(
        update_shell_timeout(&fixture.service, true, Some(&owner)).as_deref(),
        Some("10m")
    );
    assert_eq!(
        update_shell_timeout(&fixture.service, false, Some(&owner)),
        None
    );
    assert_eq!(update_shell_timeout(&fixture.service, true, None), None);
    assert_eq!(
        update_shell_timeout(&fixture.service, true, Some(&shell_tab)),
        None
    );
    assert_eq!(
        update_shell_timeout(&fixture.service, true, Some("mesh:peer/missing")),
        None
    );

    let settings_shell = fixture
        .service
        .spawn_visible_shell_impl(SpawnTabSpec {
            timeout: update_shell_timeout(&fixture.service, true, None),
            purpose: crate::multiplexer::types::ShellPurpose::Deploy,
            creator_tab_id: None,
            title: Some("CliUpdate".into()),
            ..Default::default()
        })
        .unwrap();
    assert!(fixture
        .service
        .utility_deadline_snapshot(&settings_shell.tab_id)
        .is_none());
    fixture.service.close_tab(&settings_shell.tab_id).unwrap();

    let agent_shell = fixture
        .service
        .spawn_visible_shell_impl(SpawnTabSpec {
            timeout: update_shell_timeout(&fixture.service, true, Some(&owner)),
            purpose: crate::multiplexer::types::ShellPurpose::Deploy,
            creator_tab_id: Some(owner),
            title: Some("CliUpdate".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        fixture
            .service
            .utility_deadline_snapshot(&agent_shell.tab_id)
            .unwrap()["duration_ms"],
        600_000
    );
    fixture.service.close_tab(&agent_shell.tab_id).unwrap();
}

#[test]
fn failure_tail_captures_actionable_lines_and_strips_prompt_and_env_prefix() {
    let screen = "\n$env:Path = 'C:\\Windows\\System32;' + $env:Path; npm install -g fixture-agent-pkg\nnpm ERR! code ENOTFOUND\nnpm ERR! network request to https://registry.npmjs.org/fixture-agent-pkg failed\nPS C:\\Users\\fixture>\n";
    let tail = extract_failure_tail(screen).expect("tail");
    assert!(tail.contains("npm ERR! code ENOTFOUND"), "{tail}");
    assert!(tail.contains("registry.npmjs.org"), "{tail}");
    assert!(!tail.contains("$env:Path"), "{tail}");
    assert!(!tail.contains("PS C:\\Users\\fixture>"), "{tail}");

    let summary = completion_summary_with_output(
        "fixture",
        CliOperation::Install,
        Some(ExecCompletion::CommandFinished { exit_code: Some(1) }),
        None,
        Some(&tail),
    );
    assert!(summary.contains("exit code 1"), "{summary}");
    assert!(summary.contains("npm ERR! code ENOTFOUND"), "{summary}");
}
