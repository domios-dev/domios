//! `domios secret request|status|cancel` — owner-submitted secure input coordination.
//!
//! Creates and checks typed status for masked owner input requests. Never accepts,
//! forwards, or prints any secret value.

use crate::{
    args::secret::{SecretAction, SecretInputKindArg},
    cmd::print::{fail, fail_http, print_json},
};

pub fn handle(json_mode: bool, action: SecretAction) {
    let client = crate::build_local_api().unwrap_or_else(|error| fail(1, error.to_string()));
    let request = match action {
        SecretAction::Request {
            machine,
            target,
            kind,
            reason,
            overwrite,
            provider,
            login_id,
            session_id,
            nonce,
            epoch,
            tab,
            method,
            ttl_secs,
        } => {
            if let Err(err) = validate_target_syntax(&target) {
                fail(2, err);
            }
            let payload = build_request_body(
                machine, target, kind, reason, overwrite, provider, login_id, session_id, nonce,
                epoch, tab, method, ttl_secs,
            );
            client.post("/api/secure-input/requests").json(&payload)
        }
        SecretAction::Status { request_id } => {
            let encoded = encode_segment(&request_id);
            client.get(&format!("/api/secure-input/requests/{encoded}"))
        }
        SecretAction::Cancel { request_id, reason } => {
            let encoded = encode_segment(&request_id);
            let mut body = serde_json::Map::new();
            if let Some(reason) = reason {
                body.insert("reason".to_owned(), serde_json::Value::String(reason));
            }
            client
                .post(&format!("/api/secure-input/requests/{encoded}/cancel"))
                .json(&serde_json::Value::Object(body))
        }
    };

    let response = request
        .send()
        .unwrap_or_else(|error| fail(1, error.to_string()));
    let status = response.status();
    let body = response
        .text()
        .unwrap_or_else(|error| fail(1, error.to_string()));
    if !status.is_success() {
        fail_http(1, status.as_u16(), &body);
    }
    let value: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|_| fail(1, "invalid secure input response"));
    if json_mode {
        print_json(&value);
    } else {
        println!("{}", render_human(&value));
    }
}

#[allow(clippy::too_many_arguments)]
fn build_request_body(
    machine: Option<String>,
    target: String,
    kind: SecretInputKindArg,
    reason: String,
    overwrite: bool,
    provider: Option<String>,
    login_id: Option<String>,
    session_id: Option<String>,
    nonce: Option<String>,
    epoch: Option<u64>,
    tab: Option<String>,
    method: Option<String>,
    ttl_secs: Option<u64>,
) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    if let Some(m) = machine {
        map.insert("machine".to_owned(), serde_json::Value::String(m));
    }
    map.insert("target".to_owned(), serde_json::Value::String(target));
    map.insert(
        "kind".to_owned(),
        serde_json::Value::String(kind.as_wire().to_owned()),
    );
    map.insert("reason".to_owned(), serde_json::Value::String(reason));
    if overwrite {
        map.insert("overwrite".to_owned(), serde_json::Value::Bool(true));
    }
    if let Some(v) = provider {
        map.insert("providerId".to_owned(), serde_json::Value::String(v));
    }
    if let Some(v) = login_id {
        map.insert("loginId".to_owned(), serde_json::Value::String(v));
    }
    if let Some(v) = session_id {
        map.insert("sessionId".to_owned(), serde_json::Value::String(v));
    }
    if let Some(v) = nonce {
        map.insert("nonce".to_owned(), serde_json::Value::String(v));
    }
    if let Some(v) = epoch {
        map.insert("epoch".to_owned(), serde_json::json!(v));
    }
    if let Some(v) = tab {
        map.insert("tabId".to_owned(), serde_json::Value::String(v));
    }
    if let Some(v) = method {
        map.insert("methodId".to_owned(), serde_json::Value::String(v));
    }
    if let Some(v) = ttl_secs {
        map.insert("ttlSecs".to_owned(), serde_json::json!(v));
    }
    serde_json::Value::Object(map)
}

pub(crate) fn validate_target_syntax(raw: &str) -> Result<(), String> {
    let trimmed = raw.trim();
    if let Some(name) = trimmed.strip_prefix("vault:") {
        if name.is_empty() {
            return Err("Target vault:<NAME> requires a slot name".into());
        }
        return Ok(());
    }
    if let Some(tab) = trimmed.strip_prefix("provider-login:") {
        if tab.is_empty() {
            return Err("Target provider-login:<tab> requires a tab id".into());
        }
        return Ok(());
    }
    if let Some(provider) = trimmed.strip_prefix("provider-key:") {
        if provider.is_empty() {
            return Err("Target provider-key:<provider> requires a provider id".into());
        }
        return Ok(());
    }
    Err("Target must be vault:<NAME>, provider-login:<tab>, or provider-key:<provider>".into())
}

fn encode_segment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{:02X}", byte));
        }
    }
    out
}

pub(crate) fn render_human(value: &serde_json::Value) -> String {
    let id = value
        .get("requestId")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let status = value
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let machine = value
        .get("targetMachineName")
        .and_then(|v| v.as_str())
        .unwrap_or("local");
    let target = value
        .get("targetLabel")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let kind = value.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let detail = value
        .get("reasonDetail")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    let mut lines = vec![format!(
        "request_id: {id}\nstatus: {status}\nmachine: {machine}\ntarget: {target}\nkind: {kind}"
    )];
    if let Some(detail) = detail {
        lines.push(format!("detail: {detail}"));
    }
    if status == "pending" {
        lines.push("Ask the owner to fill the masked Secure Input card in Domios; poll with `domios secret status`.".into());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests;
