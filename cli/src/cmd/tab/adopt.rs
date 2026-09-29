use crate::client::SocketClient;
use crate::cmd::print::print_json;
use crate::{build_local_api, read_api_config, resolve_tab};

use super::remote_target::{classify_remote_tab_id, RemoteTabTarget};

const LOCAL_ONLY: &str = "tab adopt-agent is caller-local and cannot target a mesh peer";

pub(super) fn handle_tab_adopt_agent(
    client: &mut SocketClient,
    json: bool,
    provider: String,
    force: bool,
    tab: Option<String>,
) {
    if tab
        .as_deref()
        .is_some_and(|value| !matches!(classify_remote_tab_id(value), RemoteTabTarget::Local))
    {
        fail(
            json,
            serde_json::json!({
                "ok": false,
                "error": {"code": "tab_adopt_local_only", "message": LOCAL_ONLY},
            }),
        );
    }
    let tab_id = resolve_tab(client, tab);
    let body = serde_json::json!({
        "provider": provider,
        "force": force,
    });
    let (api_url, _) = read_api_config();
    let api = build_local_api().unwrap_or_else(|error| {
        fail(
            json,
            serde_json::json!({
                "ok": false,
                "error": {"code": "tab_adopt_transport_failed", "message": error.to_string()},
            }),
        )
    });
    match api
        .post(format!("{api_url}/api/tabs/{tab_id}/adopt-agent"))
        .json(&body)
        .send()
    {
        Ok(response) => {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            let value = serde_json::from_str::<serde_json::Value>(&text).unwrap_or_else(|_| {
                serde_json::json!({
                    "ok": false,
                    "error": {
                        "code": "tab_adopt_invalid_response",
                        "message": if text.is_empty() { format!("HTTP {status}") } else { text },
                    },
                })
            });
            if status.is_success() {
                print_adopt_result(json, &value);
            } else {
                fail(json, value);
            }
        }
        Err(error) => fail(
            json,
            serde_json::json!({
                "ok": false,
                "error": {"code": "tab_adopt_transport_failed", "message": error.to_string()},
            }),
        ),
    }
}

fn print_adopt_result(json: bool, value: &serde_json::Value) {
    if json {
        print_json(value);
        return;
    }
    let tab = value
        .get("tab_id")
        .and_then(|item| item.as_str())
        .unwrap_or("-");
    let provider = value
        .pointer("/provider/current")
        .and_then(|item| item.as_str())
        .unwrap_or("-");
    println!("Tab {tab}: adopted as {provider}");
    let previous = value
        .pointer("/provider/previous")
        .and_then(|item| item.as_str())
        .unwrap_or("unset");
    println!("identity: {previous} -> {provider}");
    let caller = value
        .get("caller_tab_id")
        .and_then(|item| item.as_str())
        .unwrap_or("owner");
    println!("caller: {caller}");
}

fn fail(json: bool, mut value: serde_json::Value) -> ! {
    if json {
        crate::cmd::error_contract::complete_error_object(&mut value);
        print_json(&value);
    } else {
        let code = value
            .pointer("/error/code")
            .and_then(|item| item.as_str())
            .unwrap_or("tab_adopt_failed");
        let message = value
            .pointer("/error/message")
            .and_then(|item| item.as_str())
            .unwrap_or("the daemon refused tab adopt-agent");
        eprintln!("Adopt failed [{code}]: {message}");
    }
    std::process::exit(1);
}
