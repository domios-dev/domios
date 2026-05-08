//! Webhook processing — template rendering and auth verification.

use std::collections::HashMap;
use serde_json::Value;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use anyhow::{Result, anyhow};

use crate::routes::{Route, WebhookAuth};

type HmacSha256 = Hmac<Sha256>;

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
    // Expand environment variables in secret
    let secret = expand_env_var(secret);

    match auth.auth_type.as_str() {
        "hmac_sha256" | "generic" => {
            let sig = signature.ok_or_else(|| anyhow!("Missing signature header"))?;
            verify_hmac_sha256(sig, payload, &secret)
        }
        "github" => {
            // GitHub format: sha256=<signature>
            let sig = signature.ok_or_else(|| anyhow!("Missing X-Hub-Signature-256 header"))?;
            verify_github_signature(sig, payload, &secret)
        }
        "stripe" => {
            // Stripe format: t=<timestamp>,v1=<signature>
            let sig = signature.ok_or_else(|| anyhow!("Missing X-Stripe-Signature header"))?;
            verify_stripe_signature(sig, timestamp, payload, &secret)
        }
        _ => Err(anyhow!("Unknown auth type: {}", auth.auth_type)),
    }
}

/// Verify HMAC-SHA256 signature.
fn verify_hmac_sha256(signature: &str, payload: &[u8], secret: &str) -> Result<()> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| anyhow!("HMAC error: {}", e))?;
    mac.update(payload);

    let expected = hex::encode(mac.finalize().into_bytes());
    let signature = signature.trim_start_matches("sha256=");

    if expected == signature {
        Ok(())
    } else {
        Err(anyhow!("Signature mismatch"))
    }
}

/// Verify GitHub webhook signature.
fn verify_github_signature(signature: &str, payload: &[u8], secret: &str) -> Result<()> {
    let signature = signature.trim_start_matches("sha256=");
    verify_hmac_sha256(signature, payload, secret)
}

/// Verify Stripe webhook signature.
fn verify_stripe_signature(
    signature: &str,
    timestamp: Option<&str>,
    payload: &[u8],
    secret: &str,
) -> Result<()> {
    let ts = timestamp.ok_or_else(|| anyhow!("Missing Stripe timestamp"))?;

    // Stripe format: t=<timestamp>,v1=<signature>
    let parts: Vec<&str> = signature.split(',').collect();
    let mut ts_val = String::new();
    let mut sig_val = String::new();

    for part in parts {
        if let Some(v) = part.strip_prefix("t=") {
            ts_val = v.to_string();
        } else if let Some(v) = part.strip_prefix("v1=") {
            sig_val = v.to_string();
        }
    }

    if ts_val != ts {
        return Err(anyhow!("Timestamp mismatch"));
    }

    // Compute expected signature
    let signed_payload = format!("{}.{}", ts_val, String::from_utf8_lossy(payload));
    verify_hmac_sha256(&sig_val, signed_payload.as_bytes(), secret)
}

/// Render a template with the given context.
pub fn render_template(template: &str, ctx: &TemplateContext) -> String {
    let mut result = template.to_string();

    // Replace {{json.path}} with JSON values
    let json_regex = regex::Regex::new(r"\{\{json\.([^\}]+)\}\}").unwrap();
    for cap in json_regex.captures_iter(template) {
        let jsonpath = &cap[1];
        let value = get_json_value(&ctx.body, jsonpath);
        result = result.replace(&cap[0], &value);
    }

    // Replace {{header.Name}} with header values
    let header_regex = regex::Regex::new(r"\{\{header\.([^\}]+)\}\}").unwrap();
    for cap in header_regex.captures_iter(&result) {
        let header_name = &cap[1].replace('_', "-");
        let value = ctx.headers.get(&header_name).unwrap_or(&"");
        result = result.replace(&cap[0], value);
    }

    // Replace {{path}} with the request path
    result = result.replace("{{path}}", &ctx.path);

    // Replace {{query.name}} with query params
    let query_regex = regex::Regex::new(r"\{\{query\.([^\}]+)\}\}").unwrap();
    for cap in query_regex.captures_iter(&result) {
        let param_name = &cap[1];
        let value = ctx.query.get(param_name).unwrap_or(&"");
        result = result.replace(&cap[0], value);
    }

    result
}

/// Get a value from JSON using a simple path expression.
/// Supports: field, nested.field, array[0], array[0].field
fn get_json_value(json: &Value, path: &str) -> String {
    let mut current = json;

    for part in path.split('.') {
        // Handle array indexing
        if let Some(arr_part) = part.strip_suffix(".[0]") {
            // Array shorthand [0]
            if let Some(arr) = current.get(arr_part).and_then(|v| v.as_array()) {
                current = match arr.first() {
                    Some(v) => v,
                    None => return String::new(),
                };
            } else {
                return String::new();
            }
        } else if let Some(idx_str) = part.strip_suffix(']') {
            // Array indexing with number
            if let Some(field_part) = idx_str.strip_suffix('[') {
                let idx: usize = match idx_str.parse() {
                    Ok(i) => i,
                    Err(_) => return String::new(),
                };
                if let Some(arr) = current.get(field_part).and_then(|v| v.as_array()) {
                    current = match arr.get(idx) {
                        Some(v) => v,
                        None => return String::new(),
                    };
                } else {
                    return String::new();
                }
            } else {
                return String::new();
            }
        } else {
            // Simple field access
            match current.get(part) {
                Some(v) => current = v,
                None => return String::new(),
            }
        }
    }

    // Convert to string
    match current {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
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

impl TemplateContext<'_> {
    pub fn new(
        body: Value,
        headers: &HashMap<String, String>,
        path: String,
        query: &HashMap<String, String>,
    ) -> Self {
        Self {
            body,
            headers,
            path,
            query,
        }
    }
}

/// Expand environment variables in a string.
/// Format: ${VAR_NAME}
fn expand_env_var(s: &str) -> String {
    let mut result = s.to_string();
    let re = regex::Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}").unwrap();

    for cap in re.captures_iter(s) {
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
        assert_eq!(
            get_json_value(&json, "data.user.email"),
            "test@example.com"
        );
    }

    #[test]
    fn test_expand_env_var() {
        std::env::set_var("TEST_VAR", "hello");
        assert_eq!(expand_env_var("${TEST_VAR}"), "hello");
        assert_eq!(expand_env_var("prefix-${TEST_VAR}-suffix"), "prefix-hello-suffix");
    }
}