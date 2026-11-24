use super::JsonRpcResponse;
use serde_json::{json, Value};

pub(super) fn authorization(
    id: Option<Value>,
    message: String,
    neighbor_runtime: bool,
) -> JsonRpcResponse {
    let (reason, class) = match message.split(':').next().unwrap_or_default() {
        "A local API credential is required" => ("missing_token", "credential"),
        "Invalid or revoked local API credential" => ("invalid_token", "credential"),
        reason => match neighbor_class(reason) {
            Some(class) => (reason, class),
            None => ("binding_refused", "binding"),
        },
    };
    log::debug!("[socket-auth] refused class={class}");
    let data = neighbor_runtime.then(|| json!({"reason":reason}));
    let mut response = JsonRpcResponse::error(id, -32003, message);
    if let Some(error) = response.error.as_mut().filter(|_| data.is_some()) {
        error.data = data;
    }
    response
}

pub(super) fn http(status: u16, body: &[u8]) {
    if log::log_enabled!(log::Level::Debug) {
        let detail: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
        let reason = detail["code"]
            .as_str()
            .or_else(|| detail["error"]["code"].as_str());
        let class = reason.and_then(neighbor_class).unwrap_or(match status {
            401 => "credential",
            403 => "grant",
            _ => "request",
        });
        log::debug!("[socket-auth] Neighbor runtime refused class={class} http_status={status}");
    }
}

fn neighbor_class(reason: &str) -> Option<&'static str> {
    Some(match reason {
        "neighbor_forbidden" | "friendship_ended" | "friendship_not_found" => "binding",
        "session_revoked" => "credential",
        "receiver_status_unknown"
        | "receiver_not_chosen"
        | "receiving_elsewhere"
        | "receiver_elsewhere"
        | "not_receiver"
        | "recipient_not_receiving"
        | "key_revoked"
        | "key_inventory_pending"
        | "installation_unverified" => "receiver",
        "neighbor_permission_denied"
        | "neighbor_capability_denied"
        | "neighbor_paused"
        | "neighbor_restore_grant_required"
        | "neighbor_secrets_not_allowed"
        | "tab_requests_not_allowed"
        | "neighbors_disabled"
        | "friends_locked"
        | "recipient_locked"
        | "recipient_paused" => "grant",
        "reply_target_unknown" | "reply_target_stopped" => "link",
        _ => return None,
    })
}
