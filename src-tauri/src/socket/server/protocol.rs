//! Control-plane protocol gate for socket and mesh RPC dispatch.
//!
//! One integer, one typed error: dispatch refuses a client presenting an older
//! integer with `VERSION_SKEW` carrying both versions and the recovery command.
//! Reverting the gate call sites restores pre-gate dispatch; the error type is
//! additive and carries no persisted state.

use super::JsonRpcError;

/// Bump together with `cli/src/protocol.rs` on a breaking CLI↔daemon wire
/// contract change — see essences/contributing/cli.md (consumer audit).
pub(crate) const PROTOCOL_VERSION: u64 = 1;

/// JSON-RPC server-error-band code for a protocol version mismatch.
pub(crate) const VERSION_SKEW: i32 = -32002;

pub(crate) const RECOVERY_COMMAND: &str = if cfg!(debug_assertions) {
    "./build.sh cli"
} else {
    "codeterm update install --auto"
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
mod tests {
    use super::*;

    #[test]
    fn version_skew_mismatch_round_trips_both_versions_and_recovery() {
        let error = version_skew_gate(4, 9, "recover --now").expect("older client is refused");

        assert_eq!(error.code, VERSION_SKEW);
        let data = error.data.expect("typed error carries structured data");
        assert_eq!(data["type"], "version_skew");
        assert_eq!(data["client_protocol"], 4);
        assert_eq!(data["server_protocol"], 9);
        assert_eq!(data["recovery"], "recover --now");
        assert!(error.message.contains("4"));
        assert!(error.message.contains("9"));
        assert!(error.message.contains("recover --now"));
    }

    #[test]
    fn version_skew_same_version_gate_is_inert() {
        assert!(version_skew_gate(PROTOCOL_VERSION, PROTOCOL_VERSION, RECOVERY_COMMAND).is_none());
    }

    #[test]
    fn version_skew_newer_client_is_not_refused() {
        assert!(
            version_skew_gate(PROTOCOL_VERSION + 1, PROTOCOL_VERSION, RECOVERY_COMMAND).is_none()
        );
    }

    #[test]
    fn version_skew_absent_or_malformed_client_version_is_treated_as_oldest() {
        assert_eq!(client_protocol_version(None), 0);
        for malformed in [
            serde_json::json!("newest"),
            serde_json::json!(-3),
            serde_json::json!(1.5),
            serde_json::json!(null),
            serde_json::json!({"protocol_version": 1}),
        ] {
            assert_eq!(client_protocol_version(Some(&malformed)), 0);
        }

        let error = version_skew_gate(
            client_protocol_version(None),
            PROTOCOL_VERSION,
            RECOVERY_COMMAND,
        )
        .expect("oldest client receives the typed error, not a crash");
        let data = error.data.expect("typed error carries structured data");
        assert_eq!(data["client_protocol"], 0);
        assert_eq!(data["server_protocol"], PROTOCOL_VERSION);
        assert_eq!(data["recovery"], RECOVERY_COMMAND);
    }
}
