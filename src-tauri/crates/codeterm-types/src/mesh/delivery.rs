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

// ── Sender hop (outbound `PeerClient::send_relay`) ──────────────────────────
// The codes above classify what the RECEIVING peer did with a relayed command.
// These classify the SENDER's own transport leg — did the request ever leave
// this instance before the failure — so a caller can tell nothing-landed from
// landed-unknown before deciding whether a retry is safe.

/// The connection itself failed: refused, DNS failure, TLS handshake failure.
/// The only case where the request is provably never written. Nothing was sent.
pub const MESH_TRANSPORT_NOTHING_SENT: &str = "mesh_transport_nothing_sent";
/// Anything past a successful connect — a timeout, or a connection reset
/// mid-request. The payload may already have been written and be in flight —
/// never treat this as nothing-landed.
pub const MESH_TRANSPORT_SUBMIT_UNCONFIRMED: &str = "mesh_transport_submit_unconfirmed";
/// The peer answered the relay POST itself with a non-success status (e.g. its
/// own auth check rejected before dispatch). The transport hop landed; whether
/// the relayed command ran is a separate, peer-side question.
pub const MESH_TRANSPORT_PEER_REJECTED: &str = "mesh_transport_peer_rejected";
/// The peer answered with a success status but the response body could not be
/// parsed as a relay response.
pub const MESH_TRANSPORT_BAD_RESPONSE: &str = "mesh_transport_bad_response";
