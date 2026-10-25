use crate::args::blueprint::BlueprintWebhookAction;
use crate::cmd::print::{fail, print_json};
use serde_json::{json, Value};

fn request(action: &BlueprintWebhookAction) -> Result<(&str, Value), String> {
    use BlueprintWebhookAction::*;
    let (id, operation, reveal) = match action {
        Enable { id, reveal } => (id, "enable", *reveal),
        Rotate { id, reveal } => (id, "rotate", *reveal),
        Disable { id } => (id, "disable", false),
        ShowUrl { id } => (id, "show-url", false),
    };
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err("Use the Blueprint id from blueprint list".into());
    }
    Ok((
        id,
        json!({
            "action":operation,"reveal":reveal,
            "caller_tab_id":std::env::var("CODETERM_TAB_ID").ok(),
            "session_binding_nonce":std::env::var("CODETERM_SESSION_BINDING_NONCE").ok(),
        }),
    ))
}

pub(super) fn handle(json_mode: bool, action: &BlueprintWebhookAction) {
    let (id, body) = request(action).unwrap_or_else(|e| fail(2, e));
    let (base, _) = crate::read_api_config();
    let api = crate::build_local_api().unwrap_or_else(|_| fail(1, "Local API unavailable"));
    let response = api
        .post(&format!("{base}/api/blueprints/{id}/webhook/configure"))
        .json(&body)
        .send()
        .unwrap_or_else(|_| fail(1, "Webhook configuration request failed"));
    if !response.status().is_success() {
        fail(
            1,
            format!(
                "Webhook configuration refused (HTTP {})",
                response.status().as_u16()
            ),
        );
    }
    let mut value: Value = response
        .json()
        .unwrap_or_else(|_| fail(1, "Invalid webhook configuration receipt"));
    if let Some(path) = value["path"].as_str() {
        value["url"] = json!(format!("{base}{path}"));
    }
    if json_mode {
        print_json(&value);
    } else {
        if let Some(url) = value["url"].as_str() {
            println!("{url}");
        }
        if let Some(secret) = value["secret"].as_str() {
            println!("{secret}");
        }
        if value["enabled"] == false {
            println!("Webhook disabled");
        }
    }
}

#[cfg(test)]
mod tests;
