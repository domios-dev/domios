use super::{classify_remote_tab_id, relay_tab_json, RemoteTabTarget, INVALID_REMOTE_TAB_ID};
use crate::client::SocketClient;
use crate::cmd::print::{exit_with_cli_error, handle_result, CliError};
use crate::{build_local_api, read_api_config, resolve_tab};

pub(super) fn handle_tab_capture(
    client: &mut SocketClient,
    json: bool,
    action: crate::args::misc::TabCaptureAction,
) {
    use crate::args::misc::TabCaptureAction;
    let selector = match &action {
        TabCaptureAction::Start { tab, .. } | TabCaptureAction::Stop { tab } => tab.as_deref(),
    };
    match selector.map(classify_remote_tab_id) {
        Some(RemoteTabTarget::Remote(peer, remote_tab)) => {
            let (suffix, body) = match action {
                TabCaptureAction::Start { out, .. } => {
                    ("/capture/start", serde_json::json!({ "out": out }))
                }
                TabCaptureAction::Stop { .. } => ("/capture/stop", serde_json::json!({})),
            };
            let path = format!("/api/tabs/{}{}", urlencoding::encode(&remote_tab), suffix);
            handle_result(
                json,
                Ok(relay_tab_json(
                    &peer,
                    "POST",
                    &path,
                    Some(&body.to_string()),
                )),
            );
            return;
        }
        Some(RemoteTabTarget::Invalid) => {
            exit_with_cli_error(json, &CliError::new(INVALID_REMOTE_TAB_ID));
        }
        _ => {}
    }
    let (api_url, _) = read_api_config();
    let http = match build_local_api() {
        Ok(client) => client,
        Err(error) => {
            crate::cmd::print::fail(1, format!("failed to build HTTP client: {error}"));
        }
    };
    let (tab, url, body) = match action {
        TabCaptureAction::Start { tab, out } => {
            let tab_id = resolve_tab(client, tab);
            let url = format!("{api_url}/api/tabs/{tab_id}/capture/start");
            (tab_id, url, serde_json::json!({ "out": out }))
        }
        TabCaptureAction::Stop { tab } => {
            let tab_id = resolve_tab(client, tab);
            let url = format!("{api_url}/api/tabs/{tab_id}/capture/stop");
            (tab_id, url, serde_json::json!({}))
        }
    };
    let _ = tab;
    match http.post(url).json(&body).send() {
        Ok(response) => {
            let value = response
                .json::<serde_json::Value>()
                .unwrap_or_else(|_| serde_json::json!({}));
            handle_result(json, Ok(value));
        }
        Err(error) => {
            crate::cmd::print::fail(1, format!("capture request failed: {error}"));
        }
    }
}
