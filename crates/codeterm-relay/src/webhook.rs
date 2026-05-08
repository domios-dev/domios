//! Webhook processing — template rendering and auth verification.

use std::collections::HashMap;
use serde_json::Value;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use anyhow::{Result, anyhow};
use once_cell::sync::Lazy;
use regex::Regex;

use crate::routes::WebhookAuth;

type HmacSha256 = Hmac<Sha256>;

/// Cached regex patterns for template rendering.
mod patterns {
    use once_cell::sync::Lazy;
    use regex::Regex;

    pub static JSON_TEMPLATE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"\{\{json\.([^\}]+)\}\}").unwrap()
    });

    pub static HEADER_TEMPLATE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"\{\{header\.([^\}]+)\}\}").unwrap()
    });

    pub static QUERY_TEMPLATE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"\{\{query\.([^\}]+)\}\}").unwrap()
    });

    pub static ENV_VAR: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}").unwrap()
    });
}

/// A processed webhook event ready to send.
#[derive(Debug)]
pub struct ProcessedEvent {
    /// The rendered message
    pub message: String,
    /// Target peer
    pub peer: String,
    /// Target pane
    pub pane: String,
    /// Route name
    pub route_name: String,
}

/// Verify webhook signature.
pub fn verify_webhook(
    auth: &WebhookAuth,
    signature: Option<&str>,
    timestamp: Option<&str>,
    payload: &[u8],
    secret: &str,
) -> Result<()> {
    let secret = expand_env_var(secret);

    match auth.auth_type.as_str() {
        "hmac_sha256" | "generic" => {
            let sig = signature.ok_or_else(|| anyhow!("Missing signature header"))?;
            verify_hmac_sha256(sig, payload, &secret)
        }
        "github" => {
            let sig = signature.ok_or_else(|| anyhow!("Missing X-Hub-Signature-256 header"))?;
            verify_github_signature(sig, payload, &secret)
        }
        "stripe" => {
            let sig = signature.ok_or_else(|| anyhow!("Missing X-Stripe-Signature header"))?;
            verify_stripe_signature(sig, timestamp, payload, &secret)
        }
        _ => Err(anyhow!("Unknown auth type: {}", auth.auth_type)),
    }
}

/// Verify HMAC-SHA256 signature.
fn verify_hmac_sha256(signature: &str, payload: &[u8], secret: &str) -> Result<()> {
    let sig = signature.trim_start_matches("sha256=");
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| anyhow!("HMAC error: {}", e))?;
    mac.update(payload);
    let expected = hex::encode(mac.finalize().into_bytes());
    if expected == sig {
        Ok(())
    } else {
        Err(anyhow!("Signature mismatch"))
    }
}

/// Verify GitHub webhook signature.
fn verify_github_signature(signature: &str, payload: &[u8], secret: &str) -> Result<()> {
    let sig = signature.trim_start_matches("sha256=");
    verify_hmac_sha256(sig, payload, secret)
}

/// Verify Stripe webhook signature.
fn verify_stripe_signature(
    signature: &str,
    timestamp: Option<&str>,
    payload: &[u8],
    secret: &str,
) -> Result<()> {
    let ts = timestamp.ok_or_else(|| anyhow!("Missing Stripe timestamp"))?;

    // Parse Stripe format: t=<timestamp>,v1=<signature>
    let mut ts_val = String::new();
    let mut sig_val = String::new();

    for part in signature.split(',') {
        if let Some(v) = part.strip_prefix("t=") {
            ts_val = v.to_string();
        } else if let Some(v) = part.strip_prefix("v1=") {
            sig_val = v.to_string();
        }
    }

    if ts_val != ts {
        return Err(anyhow!("Timestamp mismatch"));
    }

    let signed_payload = format!("{}.{}", ts_val, String::from_utf8_lossy(payload));
    verify_hmac_sha256(&sig_val, signed_payload.as_bytes(), secret)
}

/// Context for template rendering.
pub struct TemplateContext<'a> {
    /// Parsed JSON body
    pub body: Value,
    /// Request headers
    pub headers: &'a HashMap<String, String>,
    /// Request path
    pub path: String,
    /// Query parameters
    pub query: &'a HashMap<String, String>,
}

impl<'a> TemplateContext<'a> {
    pub fn new(
        body: Value,
        headers: &'a HashMap<String, String>,
        path: String,
        query: &'a HashMap<String, String>,
    ) -> Self {
        Self { body, headers, path, query }
    }
}

/// Render a template with the given context.
pub fn render_template(template: &str, ctx: &TemplateContext) -> String {
    let mut result = template.to_string();

    // Replace {{json.path}} with JSON values
    for cap in patterns::JSON_TEMPLATE.captures_iter(template) {
        let jsonpath = &cap[1];
        let value = get_json_value(&ctx.body, jsonpath);
        result = result.replace(&cap[0], &value);
    }

    // Replace {{header.Name}} with header values
    for cap in patterns::HEADER_TEMPLATE.captures_iter(&result) {
        let header_name = &cap[1].replace('_', "-");
        let value = ctx.headers.get(&header_name).unwrap_or(&"");
        result = result.replace(&cap[0], value);
    }

    // Replace {{path}} with the request path
    result = result.replace("{{path}}", &ctx.path);

    // Replace {{query.name}} with query params
    for cap in patterns::QUERY_TEMPLATE.captures_iter(&result) {
        let param_name = &cap[1];
        let value = ctx.query.get(param_name).unwrap_or(&"");
        result = result.replace(&cap[0], value);
    }

    result
}

/// Get a value from JSON using a simple path expression.
fn get_json_value(json: &Value, path: &str) -> String {
    let mut current = json;

    for part in path.split('.') {
        current = match resolve_json_part(current, part) {
            Some(v) => v,
            None => return String::new(),
        };
    }

    value_to_string(current)
}

/// Resolve a single part of a JSON path (field access or array indexing).
fn resolve_json_part<'a>(json: &'a Value, part: &str) -> Option<&'a Value> {
    // Handle array shorthand [0]
    if let Some(arr_part) = part.strip_suffix(".[0]") {
        return json.get(arr_part)
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first());
    }

    // Handle array indexing with number: field[0]
    if let Some(idx_str) = part.strip_suffix(']') {
        if let Some(field_part) = idx_str.strip_suffix('[') {
            let idx: usize = idx_str.parse().ok()?;
            return json.get(field_part)
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.get(idx));
        }
        return None;
    }

    // Simple field access
    json.get(part)
}

/// Convert a JSON value to a string.
fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// Expand environment variables in a string.
/// Format: ${VAR_NAME}
fn expand_env_var(s: &str) -> String {
    let mut result = s.to_string();

    for cap in patterns::ENV_VAR.captures_iter(s) {
        let var_name = &cap[1];
        if let Ok(value) = std::env::var(var_name) {
            result = result.replace(&cap[0], &value);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_json_value_simple() {
        let json: Value = serde_json::json!({
            "name": "test",
            "count": 42
        });
        assert_eq!(get_json_value(&json, "name"), "test");
        assert_eq!(get_json_value(&json, "count"), "42");
    }

    #[test]
    fn test_get_json_value_nested() {
        let json: Value = serde_json::json!({
            "data": {
                "user": {
                    "email": "test@example.com"
                }
            }
        });
        assert_eq!(get_json_value(&json, "data.user.email"), "test@example.com");
    }

    #[test]
    fn test_expand_env_var() {
        std::env::set_var("TEST_VAR", "hello");
        assert_eq!(expand_env_var("${TEST_VAR}"), "hello");
        assert_eq!(expand_env_var("prefix-${TEST_VAR}-suffix"), "prefix-hello-suffix");
    }

    #[test]
    fn test_render_template() {
        let json: Value = serde_json::json!({
            "name": "test-event",
            "count": 42
        });
        let headers = HashMap::from([("Content-Type".into(), "application/json".into())]);
        let ctx = TemplateContext::new(json, &headers, "/webhook/test".into(), &HashMap::new());

        let result = render_template("Event: {{json.name}}, Count: {{json.count}}", &ctx);
        assert_eq!(result, "Event: test-event, Count: 42");
    }
}
