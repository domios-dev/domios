//! CLI↔daemon control-plane protocol contract.
//!
//! Bump `PROTOCOL_VERSION` together with
//! `src-tauri/src/socket/server/protocol.rs` on a breaking wire contract
//! change — see docs/contributing/cli.md (consumer audit).

/// One integer presented on every socket and mesh RPC request envelope.
pub const PROTOCOL_VERSION: u64 = 1;

/// JSON-RPC error code the daemon returns on protocol version skew.
pub const VERSION_SKEW_CODE: i32 = -32002;

/// Version-skew errors render their daemon-composed message verbatim.
pub fn is_version_skew(code: i32) -> bool {
    code == VERSION_SKEW_CODE
}
