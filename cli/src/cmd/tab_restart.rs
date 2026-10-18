use crate::client::SocketClient;
use crate::cmd::print::handle_result;
use crate::local_api::LocalApiClient;
use crate::{build_local_api, read_api_config, resolve_tab};

pub(super) fn handle_shell_restart(
    client: &mut SocketClient,
    json: bool,
    tab: Option<String>,
    cwd: Option<String>,
    cmd: Option<String>,
    timeout: Option<String>,
) {
    let tab_id = resolve_tab(client, tab);
    let (base, _) = read_api_config();
    let api = match build_local_api() {
        Ok(api) => api,
        Err(error) => {
            handle_result(json, Err(error.to_string()));
            return;
        }
    };
    let mut body = serde_json::json!({});
    if let Some(cwd) = cwd {
        body["cwd"] = cwd.into();
    }
    if let Some(timeout) = timeout {
        body["timeout"] = timeout.into();
    }
    let result = restart_with_command(&base, &tab_id, &body, cmd.as_deref(), |url, body| {
        post_json(&api, url, body)
    });
    if let Ok(body) = &result {
        if let Some(executed) = body.get("exec") {
            let report = super::tab::exec::exec_report(executed);
            super::tab::exec::finish_exec_report(json, body.clone(), report);
        }
    }
    handle_result(json, result);
}

fn command_body(command: &str) -> serde_json::Value {
    serde_json::json!({"command": command, "timeout_ms": 10_000, "strip_ansi": true})
}

fn restart_with_command(
    base: &str,
    tab_id: &str,
    body: &serde_json::Value,
    command: Option<&str>,
    mut post: impl FnMut(&str, &serde_json::Value) -> Result<serde_json::Value, String>,
) -> Result<serde_json::Value, String> {
    if let Some(command) = command {
        codeterm_wire::shell_command::validate(command)?;
    }
    let restarted = post(&format!("{base}/api/tabs/{tab_id}/restart"), body)?;
    match command {
        Some(command) => {
            let executed = post(
                &format!("{base}/api/tabs/{tab_id}/exec"),
                &command_body(command),
            )?;
            Ok(serde_json::json!({"restart": restarted, "exec": executed}))
        }
        None => Ok(restarted),
    }
}

fn post_json(
    api: &LocalApiClient,
    url: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let response = api.post(url).json(body).send().map_err(|e| e.to_string())?;
    let status = response.status();
    let text = response.text().unwrap_or_default();
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status.as_u16(), text));
    }
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

pub(super) fn handle_timeout(json: bool, tab_id: &str, timeout: &str) {
    let (base, _) = read_api_config();
    let result = build_local_api()
        .map_err(|error| error.to_string())
        .and_then(|api| {
            post_json(
                &api,
                &format!("{base}/api/tabs/{tab_id}/timeout"),
                &serde_json::json!({"timeout": timeout}),
            )
        });
    handle_result(json, result);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn command_json_preserves_literal_backslashes_and_control_characters() {
        for command in [
            r"\b",
            r"\n",
            r"\t",
            r".\build.ps1",
            r"C:\new\test",
            "one\ntwo\tthree",
            r"\\host\build",
        ] {
            let encoded = serde_json::to_vec(&command_body(command)).unwrap();
            let decoded: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
            assert_eq!(
                decoded["command"].as_str().unwrap().as_bytes(),
                command.as_bytes()
            );
        }
    }

    #[test]
    fn restart_wires_exact_command_to_exec_and_keeps_failure_status() {
        let command = r".\build.ps1 C:\new\test \b \n \t";
        let mut calls = Vec::new();
        let result = restart_with_command("http://local", "tab", &json!({}), Some(command), |url, body| {
            calls.push((url.to_string(), body.clone()));
            Ok(if calls.len() == 1 { json!({"ok": true}) } else {
                json!({"exit_code": 1, "completion": "command_finished", "output": "failed-start"})
            })
        }).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "http://local/api/tabs/tab/restart");
        assert_eq!(calls[1].0, "http://local/api/tabs/tab/exec");
        assert_eq!(calls[1].1["command"], command);
        let report = super::super::tab::exec::exec_report(&result["exec"]);
        assert_eq!(report.exit, 1);
        assert_eq!(report.stdout, "failed-start");
    }

    #[test]
    fn failed_restart_does_not_execute_command() {
        let mut calls = 0;
        let result =
            restart_with_command("http://local", "tab", &json!({}), Some("opaque"), |_, _| {
                calls += 1;
                Err("restart refused".into())
            });
        assert_eq!(calls, 1);
        assert_eq!(result.unwrap_err(), "restart refused");
    }

    #[test]
    fn corrupted_command_is_refused_before_restarting_shell() {
        let result = restart_with_command(
            "http://local",
            "tab",
            &json!({}),
            Some(".\u{8}uild.ps1"),
            |_, _| panic!("invalid commands must not replace the shell"),
        );
        assert!(result.unwrap_err().contains("U+0008"));
    }
}
