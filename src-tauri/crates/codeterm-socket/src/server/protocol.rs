//! Control-plane protocol gate for socket and mesh RPC dispatch.
//!
//! One integer, one typed error: dispatch refuses a client presenting an older
//! integer with `VERSION_SKEW` carrying both versions and the recovery command.
//! Reverting the gate call sites restores pre-gate dispatch; the error type is
//! additive and carries no persisted state.

use super::JsonRpcError;

/// Bump together with `cli/src/protocol.rs` on a breaking CLI↔daemon wire
/// contract change — see .agents/skills/cli-contributor/SKILL.md (consumer audit).
pub(crate) const PROTOCOL_VERSION: u64 = 14;

/// JSON-RPC server-error-band code for a protocol version mismatch.
pub(crate) const VERSION_SKEW: i32 = -32002;

pub(crate) const RECOVERY_COMMAND: &str = if cfg!(debug_assertions) {
    "./build.sh cli"
} else {
    "domios update install --auto"
};

/// A missing or non-integer client version is the oldest protocol, never an error.
pub(crate) fn client_protocol_version(raw: Option<&serde_json::Value>) -> u64 {
    raw.and_then(serde_json::Value::as_u64).unwrap_or(0)
}

pub(crate) fn version_skew_gate(client: u64, server: u64, recovery: &str) -> Option<JsonRpcError> {
    (client < server).then(|| version_skew_error(client, server, recovery))
}

fn version_skew_error(client: u64, server: u64, recovery: &str) -> JsonRpcError {
    JsonRpcError {
        code: VERSION_SKEW,
        message: format!("client protocol v{client}, server requires >={server} — run {recovery}"),
        data: Some(serde_json::json!({
            "type": "version_skew",
            "client_protocol": client,
            "server_protocol": server,
            "recovery": recovery,
        })),
    }
}

#[cfg(test)]
mod tests;
