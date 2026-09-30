//! `tab exec` result reporting: the command's exit code becomes the CLI's.

use std::io::Write;

use serde_json::Value;

use crate::cmd::print::{exit_with_cli_error, print_json, write_cli_error, CliError};

/// A timed-out exec exits like coreutils `timeout`.
pub(super) const EXEC_TIMEOUT_EXIT: i32 = 124;

#[derive(Debug, PartialEq)]
pub(super) struct ExecReport {
    pub(super) stdout: String,
    pub(super) stderr: Option<String>,
    pub(super) exit: i32,
}

pub(super) fn exec_report(body: &Value) -> ExecReport {
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
    match (exit_code, completion) {
        (Some(code), _) => ExecReport {
            stdout,
            stderr: None,
            exit: i32::try_from(code).unwrap_or(1),
        },
        (None, Some("shell_exited")) => ExecReport {
            stdout,
            stderr: Some("the tab's shell exited with an unknown status".into()),
            exit: 1,
        },
        (None, _) => ExecReport {
            stdout,
            stderr: None,
            exit: 0,
        },
    }
}

pub(super) fn finish_exec(json: bool, body: Value) -> ! {
    let report = exec_report(&body);
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

pub(super) fn fail_exec(json: bool, message: String) -> ! {
    exit_with_cli_error(json, &CliError::new(format!("exec failed: {message}")))
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
}
