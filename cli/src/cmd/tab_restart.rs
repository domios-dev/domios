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
    let result = post_json(&api, &format!("{base}/api/tabs/{tab_id}/restart"), &body);
    let result = match (result, cmd) {
        (Ok(restarted), Some(command)) => post_json(
            &api,
            &format!("{base}/api/tabs/{tab_id}/exec"),
            &serde_json::json!({
                "command": command,
                "timeout_ms": 10_000,
                "strip_ansi": true,
            }),
        )
        .map(|executed| serde_json::json!({"restart": restarted, "exec": executed})),
        (other, _) => other,
    };
    handle_result(json, result);
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
