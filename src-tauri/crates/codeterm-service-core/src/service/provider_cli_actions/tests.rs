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
