//! The one error contract of the CLI: a closed set of error codes, the
//! classifier that picks one, and the `--json` envelope every failure prints.

use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{json, Value};

static JSON_MODE: AtomicBool = AtomicBool::new(false);

/// Set once from the parsed (or pre-parse scanned) `--json` flag so any failure
/// site, however deep, renders in the mode the caller asked for.
pub fn set_json_mode(json: bool) {
    JSON_MODE.store(json, Ordering::Relaxed);
}

pub fn json_mode() -> bool {
    JSON_MODE.load(Ordering::Relaxed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    NotFound,
    InvalidArgument,
    Conflict,
    RateLimited,
    Refused,
    Unavailable,
    Internal,
}

pub const ALL_ERROR_CODES: [ErrorCode; 7] = [
    ErrorCode::NotFound,
    ErrorCode::InvalidArgument,
    ErrorCode::Conflict,
    ErrorCode::RateLimited,
    ErrorCode::Refused,
    ErrorCode::Unavailable,
    ErrorCode::Internal,
];

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::InvalidArgument => "invalid_argument",
            Self::Conflict => "conflict",
            Self::RateLimited => "rate_limited",
            Self::Refused => "refused",
            Self::Unavailable => "unavailable",
            Self::Internal => "internal",
        }
    }

    fn from_wire(value: &str) -> Option<Self> {
        ALL_ERROR_CODES
            .into_iter()
            .find(|code| code.as_str() == value)
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::NotFound => "check the id with the matching list command, then retry",
            Self::InvalidArgument => "run the command with --help for its arguments",
            Self::Conflict => {
                "the target is in a state that refuses this; inspect it before retrying"
            }
            Self::RateLimited => "wait for the retry window named in the message, then retry",
            Self::Refused => "the daemon refused this request; the message names what it needs",
            Self::Unavailable => {
                "check that CodeTerm is running (`codeterm identify` shows the target)"
            }
            Self::Internal => "check the daemon log for the failure",
        }
    }
}

fn declared_code(data: &Value) -> Option<ErrorCode> {
    let wire = data.get("code").and_then(Value::as_str).or_else(|| {
        data.get("error")
            .and_then(|error| error.get("code"))
            .and_then(Value::as_str)
    })?;
    ErrorCode::from_wire(wire)
}

fn declared_hint(data: &Value) -> Option<String> {
    ["hint", "retry_hint"]
        .iter()
        .find_map(|key| data.get(*key).and_then(Value::as_str))
        .filter(|hint| !hint.trim().is_empty())
        .map(str::to_string)
}

fn code_for_http_status(status: u64) -> Option<ErrorCode> {
    match status {
        400 | 422 => Some(ErrorCode::InvalidArgument),
        401 | 403 | 423 => Some(ErrorCode::Refused),
        404 => Some(ErrorCode::NotFound),
        409 => Some(ErrorCode::Conflict),
        429 => Some(ErrorCode::RateLimited),
        502..=504 => Some(ErrorCode::Unavailable),
        _ => None,
    }
}

const NOT_FOUND_MARKERS: [&str; 7] = [
    "no doc named",
    "not found",
    "no such",
    "does not exist",
    "no service named",
    "not installed",
    "not registered",
];
const UNAVAILABLE_MARKERS: [&str; 7] = [
    "failed to connect",
    "connection refused",
    "unreachable",
    "not running",
    "socket attempt failed",
    "is not live",
    "closed the socket",
];
const INVALID_MARKERS: [&str; 8] = [
    "must be",
    "is required",
    "requires ",
    "invalid",
    "cannot combine",
    "not valid",
    "is empty",
    "nothing to",
];

/// The status of a rendered `HTTP <status>...` failure line, for errors that
/// reach the renderer as text.
fn http_status_in(message: &str) -> Option<u64> {
    let digits: String = message
        .strip_prefix("HTTP ")?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// Structured facts win (declared code, HTTP status); free text is the last resort.
pub fn classify(message: &str, rpc_code: Option<i32>, data: Option<&Value>) -> ErrorCode {
    if let Some(code) = data.and_then(declared_code) {
        return code;
    }
    let status = data
        .and_then(|data| data.get("http_status"))
        .and_then(Value::as_u64)
        .or_else(|| http_status_in(message));
    if let Some(code) = status.and_then(code_for_http_status) {
        return code;
    }
    if rpc_code == Some(-32601) {
        return ErrorCode::Unavailable;
    }
    let lower = message.to_lowercase();
    let mentions = |markers: &[&str]| markers.iter().any(|marker| lower.contains(marker));
    if mentions(&NOT_FOUND_MARKERS) {
        return ErrorCode::NotFound;
    }
    if lower.contains("rate limited") {
        return ErrorCode::RateLimited;
    }
    if mentions(&UNAVAILABLE_MARKERS) {
        return ErrorCode::Unavailable;
    }
    if matches!(rpc_code, Some(-32602) | Some(-32601)) || mentions(&INVALID_MARKERS) {
        return ErrorCode::InvalidArgument;
    }
    ErrorCode::Internal
}

/// `error.code` is the stable machine string; `error.rpc_code` keeps the raw
/// daemon code for callers that still want it.
pub fn error_object(message: &str, rpc_code: Option<i32>, data: Option<&Value>) -> Value {
    let code = classify(message, rpc_code, data);
    let hint = data
        .and_then(declared_hint)
        .unwrap_or_else(|| code.hint().to_string());
    json!({
        "code": code.as_str(),
        "message": message,
        "hint": hint,
        "rpc_code": rpc_code,
    })
}

/// Fills the fields a command-specific error object leaves out; its own code stays.
pub fn complete_error_object(envelope: &mut Value) {
    let Some(error) = envelope.get_mut("error").and_then(Value::as_object_mut) else {
        return;
    };
    if !error.get("message").is_some_and(Value::is_string) {
        error.insert("message".into(), json!("the command failed"));
    }
    if !error.get("hint").is_some_and(Value::is_string) {
        let snapshot = Value::Object(error.clone());
        let message = snapshot["message"].as_str().unwrap_or_default();
        let code = classify(message, None, Some(&snapshot));
        error.insert("hint".into(), json!(code.hint()));
    }
}

pub fn error_envelope(message: &str, rpc_code: Option<i32>, data: Option<&Value>) -> Value {
    json!({
        "ok": false,
        "kind": "error",
        "id": null,
        "data": data.cloned(),
        "hints": [],
        "error": error_object(message, rpc_code, data),
    })
}

#[cfg(test)]
mod tests;
