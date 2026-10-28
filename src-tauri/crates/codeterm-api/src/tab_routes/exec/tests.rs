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
    assert_eq!(
        super::exec_submission_error("submit_unconfirmed: no command-start mark"),
        (
            super::StatusCode::CONFLICT,
            "submit_unconfirmed: command queued; execution not confirmed; do not resend",
        )
    );
    assert_eq!(
        super::exec_submission_error("pty_unavailable: closed before write"),
        (
            super::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to write to PTY"
        )
    );
}
