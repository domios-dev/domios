use crate::client::SocketClient;
use crate::cmd::print::handle_result;
use crate::local_api::LocalApiClient;
use crate::{build_local_api, read_api_config, resolve_tab};

#[derive(Debug)]
enum RestartRequestError {
    Refused(String),
    Unconfirmed(String),
}

impl RestartRequestError {
    fn message(self) -> String {
        match self {
            Self::Refused(message) | Self::Unconfirmed(message) => message,
        }
    }
}

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
    handle_result(json, result);
}

fn restart_with_command(
    base: &str,
    tab_id: &str,
    body: &serde_json::Value,
    command: Option<&str>,
    mut post: impl FnMut(&str, &serde_json::Value) -> Result<serde_json::Value, RestartRequestError>,
) -> Result<serde_json::Value, String> {
    if let Some(command) = command {
        codeterm_wire::shell_command::validate(command)?;
    }
    let mut body = body.clone();
    if let Some(command) = command {
        body["cmd"] = command.into();
    }
    let mut response = match post(&format!("{base}/api/tabs/{tab_id}/restart"), &body) {
        Ok(response) => response,
        Err(RestartRequestError::Unconfirmed(detail)) if command.is_some() => {
            return Ok(unconfirmed_submission(tab_id, detail));
        }
        Err(error) => return Err(error.message()),
    };
    if command.is_some() && !response.is_object() {
        return Ok(unconfirmed_submission(
            tab_id,
            "invalid restart receipt; do not replay the command".into(),
        ));
    }
    if command.is_some() && response["command_submission"]["bytes_written"] == false {
        return Err(
            "shell_command_not_submitted: daemon refused the command before writing".into(),
        );
    }
    if command.is_some()
        && response["command_delivered"] != true
        && response["command_submission"]["status"] != "submit_unconfirmed"
    {
        let uncertain = unconfirmed_submission(
            tab_id,
            "restart command acknowledgement missing; do not replay the command".into(),
        );
        response["command_submission"] = uncertain["command_submission"].clone();
    }
    Ok(response)
}

fn unconfirmed_submission(tab_id: &str, detail: String) -> serde_json::Value {
    serde_json::json!({
        "tab_id": tab_id,
        "command_delivered": false,
        "command_submission": {
            "status": "submit_unconfirmed", "bytes_written": null,
            "evidence": "receipt_unavailable", "detail": detail,
            "replay_allowed": false,
        },
    })
}

fn post_json(
    api: &LocalApiClient,
    url: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, RestartRequestError> {
    let response = api
        .post(url)
        .json(body)
        .send()
        .map_err(|error| RestartRequestError::Unconfirmed(error.to_string()))?;
    let status = response.status();
    let text = response.text().unwrap_or_default();
    if !status.is_success() {
        let message = format!("HTTP {}: {}", status.as_u16(), text);
        return Err(if status.is_client_error() {
            RestartRequestError::Refused(message)
        } else {
            RestartRequestError::Unconfirmed(message)
        });
    }
    serde_json::from_str(&text).map_err(|error| RestartRequestError::Unconfirmed(error.to_string()))
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
            .map_err(RestartRequestError::message)
        });
    handle_result(json, result);
}

#[cfg(test)]
mod tests;
