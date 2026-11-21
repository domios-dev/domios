use super::{exec_output, output_since};
use codeterm_pty::pty::manager::OutputBuffer;

#[test]
fn exec_request_preserves_paths_and_refuses_decoded_backspace() {
    let command = r".\build.ps1 C:\new\test \b \n \t";
    let body: super::ExecBody =
        serde_json::from_value(serde_json::json!({"command": command})).unwrap();
    assert_eq!(body.command, command);
    let error = serde_json::from_str::<super::ExecBody>(r#"{"command":".\build.ps1"}"#)
        .err()
        .unwrap();
    assert!(error.to_string().contains("U+0008"));
}

#[test]
fn new_shell_request_preserves_paths_and_refuses_decoded_backspace() {
    let command = r".\build.ps1 C:\new\test";
    let body: crate::tab_routes::CreateShellTabBody =
        serde_json::from_value(serde_json::json!({"cmd": command})).unwrap();
    assert_eq!(body.cmd.as_deref(), Some(command));
    let error =
        serde_json::from_str::<crate::tab_routes::CreateShellTabBody>(r#"{"cmd":".\build.ps1"}"#)
            .err()
            .unwrap();
    assert!(error.to_string().contains("U+0008"));
    assert!(
        serde_json::from_str::<crate::tab_routes::CreateShellTabBody>("{}")
            .unwrap()
            .cmd
            .is_none()
    );
}

#[test]
fn exec_output_survives_a_full_retained_ring() {
    let mut buf = OutputBuffer::new(64);
    buf.append("pty", &[b'x'; 64]);
    let start = buf.total_written("pty");
    let ring_len_before = buf.get_text("pty").map_or(0, |t| t.len());
    buf.append("pty", b"resource-json");
    assert_eq!(buf.get_text("pty").map_or(0, |t| t.len()), ring_len_before);
    let (output, offset, truncated) = output_since(&buf, "pty", start);
    assert_eq!(output, "resource-json");
    assert_eq!(offset, start + 13);
    assert!(!truncated);
}

#[test]
fn exec_output_with_an_evicted_front_reports_truncation() {
    let (output, offset, truncated) = exec_output(&[b'y'; 16], 140, 100);
    assert_eq!(output.len(), 16);
    assert_eq!(offset, 140);
    assert!(truncated);
    let (_, _, truncated) = exec_output(&[b'y'; 40], 140, 100);
    assert!(!truncated);
}

#[test]
fn exec_submission_uncertainty_is_never_a_prewrite_failure() {
    let agent = super::exec_agent_tab_rejection();
    assert_eq!(agent.status, super::StatusCode::CONFLICT);
    assert_eq!(agent.body["outcome"], "refused");
    assert_eq!(agent.body["bytes_written"], false);
    assert_eq!(agent.body["error"]["code"], "exec_agent_tab_refused");

    let exited = super::exec_shell_exited_rejection();
    assert_eq!(exited.status, super::StatusCode::CONFLICT);
    assert_eq!(exited.body["outcome"], "refused");
    assert_eq!(exited.body["bytes_written"], false);
    assert_eq!(exited.body["error"]["code"], "shell_exited_refused");

    let busy = super::exec_prewrite_rejection(
        codeterm_service_core::service::ShellExecError::BusyRefused(
            "a shell command is running".into(),
        ),
    );
    assert_eq!(busy.status, super::StatusCode::CONFLICT);
    assert_eq!(busy.body["outcome"], "refused");
    assert_eq!(busy.body["bytes_written"], false);
    assert_eq!(busy.body["error"]["code"], "tab_busy_refused");
    assert!(busy.body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("a shell command is running"));

    let unavailable = super::exec_prewrite_rejection(
        codeterm_service_core::service::ShellExecError::PtyUnavailable(
            "closed before write".into(),
        ),
    );
    assert_eq!(unavailable.status, super::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(unavailable.body["outcome"], "refused");
    assert_eq!(unavailable.body["bytes_written"], false);
    assert_eq!(unavailable.body["error"]["code"], "pty_unavailable");
}

#[test]
fn exec_unconfirmed_preserves_observed_output_and_never_claims_zero_write() {
    let observed = super::ObservedOutput {
        output: "{\"status\":\"updating\"}\n".into(),
        offset: 64,
        truncated: true,
    };
    let rejection = super::exec_unconfirmed_rejection(
        observed,
        Some(codeterm_service_core::service::ExecCompletion::TargetRestarting),
    );
    assert_eq!(rejection.status, super::StatusCode::CONFLICT);
    assert_eq!(rejection.body["outcome"], "submit_unconfirmed");
    assert_eq!(rejection.body["bytes_written"], true);
    assert_eq!(rejection.body["output"], "{\"status\":\"updating\"}\n");
    assert_eq!(rejection.body["offset"], 64);
    assert_eq!(rejection.body["output_truncated"], true);
    assert_eq!(rejection.body["completion"], "target_restarting");
    assert_eq!(rejection.body["error"]["code"], "submit_unconfirmed");
    assert_eq!(rejection.body["error"]["bytes_written"], true);
    assert!(rejection.body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("do not resend"));
}

#[test]
fn exec_completion_classifies_completed_timeout_and_target_restarting_with_output() {
    let done = super::exec_completion_response(
        "pty",
        Some(codeterm_service_core::service::ExecCompletion::CommandFinished { exit_code: Some(0) }),
        super::ObservedOutput {
            output: "ok\n".into(),
            offset: 3,
            truncated: false,
        },
        None,
    )
    .unwrap();
    assert_eq!(done["outcome"], "completed");
    assert_eq!(done["bytes_written"], true);
    assert_eq!(done["exit_code"], 0);
    assert_eq!(done["output"], "ok\n");

    let timed_out = super::exec_completion_response(
        "pty",
        None,
        super::ObservedOutput {
            output: "partial".into(),
            offset: 7,
            truncated: false,
        },
        Some(true),
    )
    .unwrap();
    assert_eq!(timed_out["outcome"], "unknown");
    assert_eq!(timed_out["timeout"], true);
    assert_eq!(timed_out["output"], "partial");

    let restarting = super::exec_completion_response(
        "pty",
        Some(codeterm_service_core::service::ExecCompletion::TargetRestarting),
        super::ObservedOutput {
            output: "{\"status\":\"updating\"}\n".into(),
            offset: 22,
            truncated: false,
        },
        None,
    )
    .unwrap_err();
    assert_eq!(restarting.status, super::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(restarting.body["outcome"], "unknown");
    assert_eq!(restarting.body["bytes_written"], true);
    assert_eq!(restarting.body["completion"], "target_restarting");
    assert_eq!(restarting.body["output"], "{\"status\":\"updating\"}\n");
    assert_eq!(restarting.body["error"]["code"], "target_restarting");
    assert_eq!(restarting.body["error"]["bytes_written"], true);
}
