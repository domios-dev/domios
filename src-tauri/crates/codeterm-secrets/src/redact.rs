//! The one gate every secret error crosses before it leaves the backend
//! boundary: typed kind plus a bounded reason, never a backend payload.

use serde_json::Value;

use crate::{SecretError, SecretRef};

const MAX_REASON_CHARS: usize = 160;
const MAX_CANDIDATES: usize = 20;
const WITHHELD: &str = "backend error (details withheld)";

/// A backend reason survives only when it is short, single-line prose; anything
/// that could be structured output (JSON, quoted fields) is replaced.
pub fn bounded_reason(reason: &str) -> String {
    let reason = reason.trim();
    let looks_structured = reason
        .chars()
        .any(|c| matches!(c, '{' | '}' | '[' | ']' | '"' | '\\') || c.is_control());
    if reason.is_empty() || looks_structured || reason.chars().count() > MAX_REASON_CHARS {
        return WITHHELD.to_string();
    }
    reason.to_string()
}

impl SecretError {
    pub fn kind(&self) -> &'static str {
        match self {
            SecretError::LoggedOut => "logged_out",
            SecretError::Locked => "locked",
            SecretError::NotFound => "not_found",
            SecretError::Ambiguous { .. } => "ambiguous",
            SecretError::AlreadyExists => "already_exists",
            SecretError::BadRequest { .. } => "bad_request",
            SecretError::Backend { .. } => "backend",
        }
    }

    pub fn redacted(self) -> Self {
        match self {
            SecretError::BadRequest { message } => SecretError::BadRequest {
                message: bounded_reason(&message),
            },
            SecretError::Backend { message } => SecretError::Backend {
                message: bounded_reason(&message),
            },
            SecretError::Ambiguous { candidates } => SecretError::Ambiguous {
                candidates: candidates
                    .into_iter()
                    .take(MAX_CANDIDATES)
                    .map(|c| bounded_reason(&c))
                    .collect(),
            },
            other => other,
        }
    }
}

/// Projects one backend record onto metadata. Every other field, including any
/// value, login, notes or custom fields, is dropped here.
pub fn metadata_only(record: &Value) -> Option<SecretRef> {
    let text = |key: &str| record.get(key).and_then(Value::as_str).map(str::to_string);
    Some(SecretRef {
        item_id: text("item_id")?,
        name: text("name")?,
        folder: text("folder"),
        organization: text("organization"),
        collection: text("collection"),
        last_modified: record.get("last_modified").and_then(Value::as_i64),
    })
}

pub fn metadata_list(records: Value) -> Result<Vec<SecretRef>, SecretError> {
    match records {
        Value::Array(items) => Ok(items.iter().filter_map(metadata_only).collect()),
        _ => Err(SecretError::Backend {
            message: "backend returned an unexpected list shape".into(),
        }),
    }
}

#[cfg(test)]
mod tests;
