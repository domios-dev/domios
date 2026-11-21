//! `tab exec` result reporting: the command's exit code becomes the CLI's.

use std::io::Write;

use serde_json::Value;

use crate::cmd::print::{exit_with_cli_error, print_json, write_cli_error, CliError};

/// A timed-out exec exits like coreutils `timeout`.
pub(super) const EXEC_TIMEOUT_EXIT: i32 = 124;

#[derive(Debug, PartialEq)]
pub(crate) struct ExecReport {
    pub(crate) stdout: String,
    pub(crate) stderr: Option<String>,
    pub(crate) exit: i32,
}

pub(crate) fn exec_report(body: &Value) -> ExecReport {
    let stdout = body
        .get("output")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if body.get("timeout").and_then(Value::as_bool) == Some(true) {
        let hint = if body.get("shell_integration").and_then(Value::as_bool) == Some(false) {
            "; this shell reports no command completion"
        } else {
            ""
        };
        return ExecReport {
            stdout,
            stderr: Some(format!("exec timed out before the command finished{hint}")),
            exit: EXEC_TIMEOUT_EXIT,
        };
    }
    let exit_code = body.get("exit_code").and_then(Value::as_i64);
    let completion = body.get("completion").and_then(Value::as_str);
    let mut report = match (exit_code, completion) {
        (Some(code), _) => ExecReport {
            stdout,
            stderr: None,
            exit: i32::try_from(code).unwrap_or(1),
        },
        (None, Some("target_restarting")) => ExecReport {
            stdout,
            stderr: Some(
                "target daemon restarted mid-exec after command delivery; outcome is unknown; do not resend"
                    .into(),
            ),
            exit: 1,
        },
        (None, Some("submit_unconfirmed")) => ExecReport {
            stdout,
            stderr: Some("command queued; execution not confirmed; do not resend".into()),
            exit: 1,
        },
        (None, Some("shell_exited")) => ExecReport {
            stdout,
            stderr: Some("the tab's shell exited with an unknown status".into()),
            exit: 1,
        },
        (None, _) => ExecReport {
            stdout,
            stderr: Some("the command's exit status is unknown".into()),
            exit: 1,
        },
    };
    if body.get("output_truncated").and_then(Value::as_bool) == Some(true)
        && report.stderr.is_none()
    {
        report.stderr =
            Some("output exceeded the tab's retained buffer; its beginning is missing".into());
    }
    report
}

#[derive(Debug, PartialEq)]
pub(crate) struct ExecHttpFailure {
    pub(crate) stdout: Option<String>,
    pub(crate) message: String,
    pub(crate) retry_hint: Option<String>,
    pub(crate) outcome: Option<String>,
    pub(crate) code: Option<String>,
    pub(crate) bytes_written: Option<bool>,
    pub(crate) output_truncated: bool,
    pub(crate) data: Value,
}

pub(crate) fn classify_exec_http_error(status: u16, body: &str) -> ExecHttpFailure {
    let trimmed = body.trim();
    let parsed = serde_json::from_str::<Value>(trimmed)
        .ok()
        .filter(Value::is_object);
    let err = parsed
        .as_ref()
        .and_then(|v| v.get("error"))
        .filter(|v| !v.is_null());
    let text_of = |key: &str| {
        err.and_then(|e| e.get(key))
            .or_else(|| parsed.as_ref().and_then(|v| v.get(key)))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let code = text_of("code");
    let outcome = parsed
        .as_ref()
        .and_then(|v| v.get("outcome"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| text_of("outcome"));
    let bytes_written = err
        .and_then(|e| e.get("bytes_written"))
        .or_else(|| parsed.as_ref().and_then(|v| v.get("bytes_written")))
        .and_then(Value::as_bool);
    let retry_hint = err
        .and_then(|e| e.get("retry_hint").or_else(|| e.get("hint")))
        .and_then(Value::as_str)
        .map(str::to_string);
    let stdout = parsed
        .as_ref()
        .and_then(|v| v.get("output"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let output_truncated = parsed
        .as_ref()
        .and_then(|v| v.get("output_truncated"))
        .and_then(Value::as_bool)
        == Some(true);
    let detail = text_of("message")
        .or_else(|| err.and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| trimmed.to_string());
    let message = match (&code, detail.is_empty()) {
        (Some(code), false) => format!("{code}: {detail}"),
        (Some(code), true) => code.clone(),
        (None, false) => format!("HTTP {status}: {detail}"),
        (None, true) => format!("HTTP {status}"),
    };
    let mut data = parsed.unwrap_or_else(|| {
        if trimmed.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::json!({ "body": trimmed })
        }
    });
    data["http_status"] = serde_json::json!(status);
    ExecHttpFailure {
        stdout,
        message,
        retry_hint,
        outcome,
        code,
        bytes_written,
        output_truncated,
        data,
    }
}

pub(crate) fn finish_exec(json: bool, body: Value) -> ! {
    let report = exec_report(&body);
    finish_exec_report(json, body, report)
}

pub(crate) fn finish_exec_report(json: bool, body: Value, report: ExecReport) -> ! {
    if json {
        if let Some(message) = report.stderr {
            let error = CliError {
                message,
                code: None,
                data: Some(body),
            };
            write_cli_error(true, &error, &mut std::io::stdout(), &mut std::io::stderr());
            std::process::exit(report.exit);
        }
        print_json(&body);
    } else {
        print!("{}", report.stdout);
    }
    let _ = std::io::stdout().flush();
    if let Some(message) = report.stderr {
        eprintln!("{message}");
    }
    std::process::exit(report.exit)
}

pub(crate) fn fail_exec(json: bool, message: String) -> ! {
    exit_with_cli_error(json, &CliError::new(format!("exec failed: {message}")))
}

pub(crate) fn fail_exec_transport(json: bool, detail: &str) -> ! {
    let body = serde_json::json!({
        "ok": false,
        "outcome": codeterm_mesh::contract::delivery::OUTCOME_UNKNOWN,
        "error": {
            "code": codeterm_mesh::contract::delivery::MESH_TRANSPORT_SUBMIT_UNCONFIRMED,
            "outcome": codeterm_mesh::contract::delivery::OUTCOME_UNKNOWN,
            "message": format!("request failed after dispatch: {detail}"),
            "retry_hint": "the exec outcome is unknown — the daemon may have restarted mid-exec; inspect the tab before resending; never replay blindly",
        }
    })
    .to_string();
    fail_exec_http(json, 504, &body)
}

pub(crate) fn fail_exec_http(json: bool, status: u16, body: &str) -> ! {
    let failure = classify_exec_http_error(status, body);
    if json {
        let mut error = serde_json::json!({
            "message": format!("exec failed: {}", failure.message),
            "code": failure
                .code
                .clone()
                .unwrap_or_else(|| crate::cmd::error_contract::classify(&failure.message, None, Some(&failure.data)).as_str().into()),
            "hint": failure
                .retry_hint
                .clone()
                .unwrap_or_else(|| crate::cmd::error_contract::classify(&failure.message, None, Some(&failure.data)).hint().into()),
        });
        if let Some(outcome) = &failure.outcome {
            error["outcome"] = serde_json::json!(outcome);
        }
        if let Some(written) = failure.bytes_written {
            error["bytes_written"] = serde_json::json!(written);
        }
        if let Some(output) = &failure.stdout {
            error["output"] = serde_json::json!(output);
        }
        if failure.output_truncated {
            error["output_truncated"] = serde_json::json!(true);
        }
        let envelope = serde_json::json!({
            "ok": false,
            "kind": "error",
            "id": serde_json::Value::Null,
            "result": failure.data,
            "hints": [],
            "error": error,
        });
        println!("{}", crate::cmd::output::encode(&envelope).unwrap());
        std::process::exit(1);
    }
    if let Some(output) = failure.stdout.as_deref().filter(|s| !s.is_empty()) {
        print!("{output}");
        let _ = std::io::stdout().flush();
    }
    if let Some(outcome) = failure.outcome.as_deref() {
        eprintln!("Error: exec failed [{outcome}]: {}", failure.message);
    } else {
        eprintln!("Error: exec failed: {}", failure.message);
    }
    if let Some(hint) = failure.retry_hint.as_deref() {
        eprintln!("hint: {hint}");
    }
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn exec_report_carries_the_command_exit_code() {
        let report = exec_report(&json!({
            "output": "opaque-output",
            "exit_code": 3,
            "completion": "command_finished",
        }));
        assert_eq!(report.exit, 3);
        assert_eq!(report.stdout, "opaque-output");
        assert!(report.stderr.is_none());
    }

    #[test]
    fn exec_report_warns_when_the_output_front_was_truncated() {
        let report = exec_report(&json!({
            "output": "tail-only",
            "exit_code": 0,
            "completion": "command_finished",
            "output_truncated": true,
        }));
        assert_eq!(report.exit, 0);
        assert_eq!(report.stdout, "tail-only");
        assert!(report.stderr.is_some());
    }

    #[test]
    fn exec_report_fails_a_timeout() {
        let report = exec_report(&json!({
            "output": "",
            "exit_code": null,
            "completion": "timeout",
            "timeout": true,
            "shell_integration": true,
        }));
        assert_eq!(report.exit, EXEC_TIMEOUT_EXIT);
        assert!(report.stderr.is_some());
    }

    #[test]
    fn exec_report_fails_a_shell_exit_with_unknown_status() {
        let report = exec_report(&json!({
            "output": "",
            "exit_code": null,
            "completion": "shell_exited",
        }));
        assert_eq!(report.exit, 1);
    }
    #[test]
    fn exec_report_never_succeeds_without_an_exit_status() {
        for completion in [
            "command_finished",
            "unknown",
            "target_restarting",
            "submit_unconfirmed",
        ] {
            let report = exec_report(&json!({"exit_code": null, "completion": completion}));
            assert_eq!(report.exit, 1);
            assert!(report.stderr.is_some());
        }
    }

    #[test]
    fn classify_exec_http_error_preserves_outcome_output_and_do_not_resend_hint() {
        let body = json!({
            "ok": false,
            "outcome": "submit_unconfirmed",
            "bytes_written": true,
            "output": "{\"status\":\"updating\"}\n",
            "output_truncated": true,
            "error": {
                "code": "submit_unconfirmed",
                "outcome": "submit_unconfirmed",
                "bytes_written": true,
                "message": "command queued; execution not confirmed; do not resend",
                "retry_hint": "never replay blindly"
            }
        })
        .to_string();
        let failure = classify_exec_http_error(409, &body);
        assert_eq!(failure.outcome.as_deref(), Some("submit_unconfirmed"));
        assert_eq!(failure.code.as_deref(), Some("submit_unconfirmed"));
        assert_eq!(failure.bytes_written, Some(true));
        assert_eq!(
            failure.stdout.as_deref(),
            Some("{\"status\":\"updating\"}\n")
        );
        assert!(failure.output_truncated);
        assert_eq!(failure.retry_hint.as_deref(), Some("never replay blindly"));
        assert!(failure.message.contains("submit_unconfirmed"));
    }
}
