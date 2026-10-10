use codeterm_types::agent::capability_probe::*;
use serde_json::Value;

pub(super) const REQUEST: &[u8] = b"{\"type\":\"control_request\",\"request_id\":\"probe-init\",\"request\":{\"subtype\":\"initialize\",\"hooks\":null}}\n";
pub(super) fn args() -> [&'static str; 14] {
    [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--settings",
        "{\"disableAllHooks\":true}",
        "--strict-mcp-config",
        "--mcp-config",
        "{\"mcpServers\":{}}",
        "--no-session-persistence",
        "--allowedTools",
        "",
    ]
}
pub(super) fn inherited_key_allowed(key: &std::ffi::OsStr) -> bool {
    let key = key.to_string_lossy().to_ascii_uppercase();
    !key.starts_with("CODETERM_") && key != "FORCE_CODE_TERMINAL"
}
fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_string)
}
fn commands(value: &Value) -> Option<Vec<ProbeCommand>> {
    Some(
        value
            .as_array()?
            .iter()
            .filter_map(|item| {
                Some(ProbeCommand {
                    name: text(item, "name")?,
                    description: text(item, "description"),
                    argument_hint: text(item, "argumentHint"),
                })
            })
            .collect(),
    )
}
pub(super) fn decode(value: &Value) -> Option<Result<InitializeCapabilities, ProbeFailure>> {
    if value["type"] != "control_response" || value["response"]["request_id"] != "probe-init" {
        return None;
    }
    let response = &value["response"];
    if response["subtype"] != "success" {
        return Some(Err(ProbeFailure::Protocol));
    }
    let data = &response["response"];
    if !data.is_object() {
        return Some(Err(ProbeFailure::Protocol));
    }
    Some(Ok(InitializeCapabilities {
        commands: commands(&data["commands"]),
        agents: commands(&data["agents"]),
        models: data["models"].as_array().map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(ProbeModel {
                        id: text(item, "value").or_else(|| text(item, "id"))?,
                        display_name: text(item, "displayName"),
                        description: text(item, "description"),
                    })
                })
                .collect()
        }),
        account: data["account"].as_object().map(|_| ProbeAccount {
            subscription_type: text(&data["account"], "subscriptionType"),
            api_provider: text(&data["account"], "apiProvider"),
        }),
        current_permission_mode: text(data, "current_permission_mode"),
        fast_mode_state: text(data, "fast_mode_state"),
        output_style: text(data, "output_style"),
    }))
}
