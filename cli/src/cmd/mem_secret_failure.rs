//! Maps a secrets API failure to a typed exit and a bounded message. The
//! daemon may be older than this CLI, so its message is never trusted raw.

use crate::cmd::print::CliError;

const MAX_MESSAGE_CHARS: usize = 160;
const WITHHELD: &str = "secret store: backend error (details withheld)";

// Stable script exit codes: `3`=locked, `4`=logged-out, `5`=not found, `6`=backend unavailable.
pub fn exit_for_status(status: u16) -> Option<i32> {
    match status {
        200 | 201 | 204 => None,
        401 => Some(4),
        423 => Some(3),
        404 => Some(5),
        503 | 0 => Some(6),
        _ => Some(1),
    }
}

fn bounded(message: &str) -> Option<String> {
    let message = message.trim();
    let structured = message
        .chars()
        .any(|c| matches!(c, '{' | '}' | '[' | ']' | '"' | '\\') || c.is_control());
    (!message.is_empty() && !structured && message.chars().count() <= MAX_MESSAGE_CHARS)
        .then(|| message.to_string())
}

pub fn secret_failure(status: u16, body: &serde_json::Value) -> Option<(i32, CliError)> {
    let code = exit_for_status(status)?;
    if status == 403 {
        let error = CliError {
            message: body
                .pointer("/error/message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .into(),
            code: None,
            data: Some(serde_json::json!({"http_status":403,"reason":body.get("reason")})),
        };
        if let Some(refusal) = error.neighbor_refusal() {
            return Some((1, refusal));
        }
    }
    let error = body.get("error");
    let kind = error
        .and_then(|err| err.get("kind"))
        .and_then(|k| k.as_str())
        .unwrap_or("error");
    let fallback = match kind {
        "logged_out" => "secret store: logged out — run `domios mem secret unlock`",
        "locked" => "secret store: vault locked — run `domios mem secret unlock`",
        "not_found" => "secret store: not found",
        "already_exists" => "secret store: already exists (use --overwrite to replace)",
        _ if error.is_none() => "",
        _ => WITHHELD,
    };
    let message = error
        .and_then(|err| err.get("message"))
        .and_then(|m| m.as_str())
        .and_then(bounded)
        .unwrap_or_else(|| {
            if fallback.is_empty() {
                format!("HTTP {status}")
            } else {
                fallback.to_string()
            }
        });
    let mut data = serde_json::json!({ "http_status": status, "secret_kind": kind });
    if status == 0 {
        data["code"] = serde_json::json!("unavailable");
    }
    Some((
        code,
        CliError {
            message,
            code: None,
            data: Some(data),
        },
    ))
}

pub(super) fn transport_failure(
    error: &crate::local_api::LocalApiError,
) -> (u16, serde_json::Value) {
    if let Some(refusal) = error.neighbor_refusal() {
        return (
            403,
            serde_json::json!({"reason":refusal.data.as_ref().and_then(|data| data.get("reason")),"error":{"kind":"refused","message":refusal.message}}),
        );
    }
    (
        0,
        serde_json::json!({"error":{"kind":"backend","message":error.to_string()}}),
    )
}

#[cfg(test)]
mod tests;
