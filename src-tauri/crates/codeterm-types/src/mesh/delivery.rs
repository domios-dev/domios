//! Delivery outcome codes carried verbatim across the mesh relay boundary.
//! Defined once for the daemon that emits them and the CLI that classifies
//! them, so both sides assert on a constant rather than on a message.

/// Target tab does not resolve on the peer. Nothing was written.
pub const TAB_NOT_REACHABLE_ON_PEER: &str = "tab_not_reachable_on_peer";
/// Target tab resolved but refused the write before any byte left. Nothing written.
pub const TAB_BUSY_REFUSED: &str = "tab_busy_refused";
/// Target tab resolved but has no usable PTY. Nothing was written.
pub const PTY_UNAVAILABLE: &str = "pty_unavailable";
/// Bytes reached the PTY but submission was never confirmed — the payload may
/// already be parked in the composer. The only code that admits a partial write.
pub const SUBMIT_UNCONFIRMED: &str = "submit_unconfirmed";
/// Programmatic payload refused before any delivery machinery ran. Nothing written.
pub const PAYLOAD_TOO_LARGE: &str = "payload_too_large";

/// In-flight delivery window saturated; the send was never dispatched.
pub const MESH_DELIVERY_WINDOW_SATURATED: &str = "mesh_delivery_window_saturated";
/// The relay abandoned its own loopback call before the delivery route answered.
/// The outcome is UNKNOWN — `bytes_written` is absent here, never `false`.
pub const RELAY_DELIVERY_DEADLINE_EXCEEDED: &str = "relay_delivery_deadline_exceeded";
/// The relay never reached the peer's own local API. Nothing was written.
pub const PEER_LOCAL_API_UNREACHABLE: &str = "peer_local_api_unreachable";
/// The sending peer is not authorized for this route. Nothing was written.
pub const PEER_RELAY_NOT_AUTHORIZED: &str = "peer_relay_not_authorized";
/// Relay protocol versions differ between the peers. Nothing was written.
pub const PEER_RELAY_PROTOCOL_MISMATCH: &str = "peer_relay_protocol_mismatch";
