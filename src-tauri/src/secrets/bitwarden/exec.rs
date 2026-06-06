//! `bw` exec layer — trait + types + error mapping.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::secrets::SecretError;

/// One execution of `bw <args>`. Tests stub this to feed canned JSON
/// without spawning a process.
#[async_trait]
pub trait BwExec: Send + Sync {
    /// Run `bw <args>` with optional stdin + extra env. Returns the parsed
    /// response wrapper. Implementations always pass `--nointeraction
    /// --response` at the end of `args`.
    async fn run(&self, ctx: BwRunCtx<'_>) -> Result<BwResponse, String>;
}

/// Inputs to a single `bw` invocation. Borrowed strings keep the call site
/// terse without cloning every arg.
pub struct BwRunCtx<'a> {
    pub args: Vec<&'a str>,
    pub stdin: Option<String>,
    pub env: HashMap<String, String>,
    pub session: Option<&'a str>,
}

/// Parsed `--response` JSON wrapper.
/// `bw` shapes:
///   success: { "success": true,  "data": <value>          }
///   failure: { "success": false, "data": null, "message": "...", "errorCode": ... }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BwResponse {
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub data: serde_json::Value,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default, rename = "errorCode")]
    pub error_code: Option<i64>,
}

impl BwResponse {
    pub fn into_data(self) -> Result<serde_json::Value, SecretError> {
        if self.success {
            Ok(self.data)
        } else {
            Err(map_bw_message(self.message.as_deref().unwrap_or("")))
        }
    }
}

/// Translate a `bw` error message into a `SecretError`. Substring matching
/// is the documented vendor-stable signal — Bitwarden has used these
/// strings for years and they survive UI rewrites.
pub fn map_bw_message(msg: &str) -> SecretError {
    let lower = msg.to_lowercase();
    if lower.contains("vault is locked") {
        SecretError::Locked
    } else if lower.contains("you are not logged in") {
        SecretError::LoggedOut
    } else if lower.contains("not found") {
        SecretError::NotFound
    } else if lower.contains("more than one result") {
        SecretError::Ambiguous { candidates: vec![] }
    } else if lower.contains("already") && lower.contains("exists") {
        SecretError::AlreadyExists
    } else {
        SecretError::Backend {
            message: if msg.is_empty() {
                "bw failed".into()
            } else {
                msg.into()
            },
        }
    }
}

/// Sentinel exec used when bw is not on PATH — every call errors with the
/// reason captured in BitwardenStore.unavailable_reason.
pub(super) struct NoopBwExec;

#[async_trait]
impl BwExec for NoopBwExec {
    async fn run(&self, _ctx: BwRunCtx<'_>) -> Result<BwResponse, String> {
        Err("bw not available".into())
    }
}
