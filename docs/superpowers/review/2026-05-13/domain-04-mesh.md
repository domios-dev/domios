# Domain Review: Mesh Subsystem

## Files Reviewed

- `src-tauri/src/mesh/mod.rs` (768 lines)
- `src-tauri/src/mesh/types.rs`
- `src-tauri/src/mesh/identity.rs`
- `src-tauri/src/mesh/crown.rs`
- `src-tauri/src/mesh/peers.rs`
- `src-tauri/src/mesh/discovery.rs` (699 lines)
- `src-tauri/src/mesh/pairing.rs`
- `src-tauri/src/mesh/transport.rs`
- `src-tauri/src/mesh/ping_scheduler.rs`
- `src-tauri/src/api/mesh_routes/mod.rs`
- `src-tauri/src/api/mesh_routes/pairing.rs`
- `src-tauri/src/api/mesh_routes/connection.rs`
- `src-tauri/src/api/mesh_routes/relay.rs`
- `src-tauri/src/api/mesh_routes/announce.rs`
- `src-tauri/src/api/mesh_routes/introduction.rs`
- `src-tauri/src/commands_mesh.rs`

---

## Findings

### src-tauri/src/mesh/crown.rs

#### OK

Nonce is generated (`uuid::Uuid::new_v4()`) and included in the signed payload; timestamp window is checked (300s past, 30s future); Ed25519 sign/verify is correct; no `unwrap()` outside test code. The nonce field is present in the token and signed, which prevents tampering with the payload, but see CRITICAL issue below regarding replay protection.

---

### src-tauri/src/mesh/mod.rs

#### CRITICAL

**`mod.rs:191` — Nonce is signed but never stored; replay attack fully viable.**
`verify_signed_request` checks timestamp (5-min window) but never records seen nonces in any set. An intercepted signed token is replayable for up to 300 seconds — every call within that window from the same peer with the same token succeeds. This affects every authenticated mesh endpoint: relay, sync, announce, unpair, introduce. The comment in `SignedRequest` says `"Nonce to prevent replay (UUID)"` but the nonce is only there to make each token unique for signing; nothing ever deduplicates received nonces.

#### HIGH

**`mod.rs:593–600` — `refresh_local_addresses` takes 4 separate lock acquisitions.**
```rust
let prev_ts = mesh.lock().tailscale_ip.clone();
let prev_lan = mesh.lock().local_ip.clone();
// ...
let new_ts = mesh.lock().tailscale_ip.clone();
let new_lan = mesh.lock().local_ip.clone();
```
Between each `.lock()` drop, another thread can mutate the state. The "prev" snapshot is not atomic with the "new" snapshot, so the changed/unchanged signal can be wrong. Races are unlikely in practice (addresses change rarely) but this is structurally wrong and the fix is a single lock scope per read pair.

**`mod.rs:252–258` — `prepare_introductions` uses a single token for all outbound introduction calls.**
The token has a 300-second lifetime and is signed once, then reused in `broadcast_introductions` across multiple peers over potentially-long network calls. If the broadcast takes longer than 300 seconds (many peers, slow routes), later introductions will present an expired token and be rejected silently (logged as warn). Token should be re-signed per-peer.

#### MEDIUM

**`mod.rs` at 768 lines — serious violation of the 300-line `mod.rs` rule.**
Logic that belongs in concrete files:
- `broadcast_sync` (lines 370–412) — free async function, belongs in `transport.rs` or a new `broadcast.rs`.
- `broadcast_introductions` (lines 419–497) — same.
- `broadcast_announce` (lines 511–586) — same.
- `refresh_local_addresses` (lines 592–608) — belongs in `discovery.rs`.
- `AnnounceReport` struct (lines 500–506) — belongs in `types.rs`.

The module should be a pure facade: type aliases, `MeshManager` struct, `impl MeshManager`, and re-exports.

**`mod.rs:39` — `pairing_targets: HashMap<String, (String, u16, String)>` never cleaned up on failure.**
`pairing_targets` is inserted when a pairing request is sent out, and removed only on successful `mesh_pair_accept`. If the remote peer rejects, times out, or the user abandons the flow, the entry stays in the map indefinitely. It is never pruned by `cleanup_expired_requests`. Over time this leaks memory and clutters state.

#### LOW

**`mod.rs:385` — `reqwest::Client::builder().build().unwrap_or_default()` in `broadcast_sync`.**
`unwrap_or_default()` creates a Client with no TLS or timeout configuration if builder fails. Same pattern repeats in `broadcast_introductions` and `broadcast_announce`. Should be `build().map_err(|e| ...)` or pre-built and passed in.

---

### src-tauri/src/mesh/discovery.rs

#### HIGH

**`discovery.rs` at 699 lines — approaching the 1000-line limit; distinct responsibilities not separated.**
Four distinct concerns live in one file:
1. mDNS daemon lifecycle (`init_mdns_daemon`, `register_mdns_service`, `unregister_mdns_service`) — could move to `mdns.rs`.
2. Tailscale / LAN IP detection (`detect_tailscale`, `find_tailscale_ip`, `try_tailscale_probe`, `detect_local_ip`) — could move to `network.rs` or `address_detect.rs`.
3. UDP broadcast threads (`start_udp_threads`, `Beacon`) — could move to `udp.rs`.
4. Orchestration entry points (`start_listeners`, `start_broadcast`, `stop_broadcast`, `cleanup_stale_peers`) — could stay in a thin `discovery.rs`.

**`discovery.rs:534` — UDP listener reads into a 1024-byte fixed buffer with no size enforcement on outer beacon size.**
`recv_from` fills up to 1024 bytes; `serde_json::from_slice` on the remainder can deserialize whatever fits. If a beacon is longer than 1024 bytes, it is silently truncated and deserialization fails (harmless). But there is no per-source rate limit: a single host on the LAN can spam beacons at line rate, causing `mesh_clone.lock().discovered_peers.insert(...)` on every packet. Combined with no cap on `discovered_peers` (see below), this is a DoS vector.

#### MEDIUM

**`discovery.rs:100,564` — `discovered_peers` has no size cap.**
Both the mDNS and UDP paths insert into `discovered_peers` without any upper bound check. On a crowded network (or with a local fuzzer), this map grows without limit. `cleanup_stale_peers` prunes peers older than 300 s, but between cleanup runs (which are called externally on a schedule not visible in this file) the map can grow arbitrarily large.

**`discovery.rs:122–126` — Two separate `mesh.lock()` calls after dropping `m` in `init_mdns_daemon`.**
```rust
mesh.lock().mdns_daemon = Some(daemon);
mesh.lock().discovery_active = true;
```
These are two atomic writes that should be one. Another thread could observe `mdns_daemon = Some` but `discovery_active = false` between the two lines.

---

### src-tauri/src/mesh/pairing.rs

#### OK

`pairing.rs` contains the X25519 / AES-256-GCM key exchange helpers. These are correctly implemented. However, the comment at line 122 says `"Legacy pairing functions removed"` — the file is now a dead-code stub. `encrypt_with_shared_secret` and `decrypt_with_shared_secret` are not called anywhere in the codebase (the current pairing flow only exchanges Ed25519 public keys). The file retains test coverage for code that is not in production use.

#### LOW

**`pairing.rs` — File's declared purpose ("X25519 + AES-256-GCM") does not match current pairing protocol.**
The module doc says pairing uses a 4-step X25519 exchange, but the actual pairing is a direct public-key exchange. The stale doc causes confusion about the security model. Either the pairing.rs cryptographic primitives should be removed, or the protocol should use them.

---

### src-tauri/src/mesh/peers.rs

#### OK

No `unwrap()` outside tests; disk I/O errors are propagated cleanly; `sanitize_addresses` is applied on load. The `load_peers` function correctly handles migration of pre-Ed25519 records.

---

### src-tauri/src/mesh/types.rs

#### MEDIUM

**`types.rs:236` — Doc comment says "Reject if >60s old" but `receive_pairing_request` (mod.rs:192) rejects at >300s.**
The field-level doc is stale by 5x. The `cleanup_expired_requests` method also uses 300 s. This is a documentation mismatch, not a security hole (300 s is more permissive than 60 s), but it erodes trust in inline documentation.

---

### src-tauri/src/mesh/identity.rs

#### OK

Thin facade over `crown` functions. No logic concerns.

---

### src-tauri/src/mesh/transport.rs

#### OK

`try_peer` correctly walks addresses in priority order; `Stop` short-circuits on authoritative responses. `unwrap_or_default()` patterns are safe (fallback HTTP clients still work, just lose timeout precision). `PeerClient::new_signed` correctly signs with Ed25519.

---

### src-tauri/src/mesh/ping_scheduler.rs

#### MEDIUM

**`ping_scheduler.rs:43` — Ping uses the best-tier address at tick-time, but uses plain HTTP with no auth.**
`/api/mesh/ping` is intentionally unauthenticated (it returns a trivial `{"mesh": true}`), so a spoofed HTTP server on any peer address can force a peer to appear permanently online. This is low-severity for normal operation but makes availability status unreliable if an adversary is in the path.

**`ping_scheduler.rs:36` — reqwest::Client built fresh every `PING_INTERVAL_SECS` (20s).**
A new client is allocated on each tick (with its own connection pool). The client should be built once and reused.

---

### src-tauri/src/api/mesh_routes/mod.rs

#### CRITICAL

**`mod.rs:106,115,125` — `.unwrap()` on `serde_json::to_value(...)` in production route handlers.**
`mesh_status`, `mesh_peers`, and `mesh_discover` all call `.unwrap()` on serialization. While `MeshStatus`, `MeshPeersResponse`, and `Vec<DiscoveredPeer>` are all `Serialize`, any future change that adds a non-serializable field (e.g., a `Box<dyn Trait>`) will cause an unrecoverable panic in a request handler, crashing the tokio worker. Should be `map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))`.

---

### src-tauri/src/api/mesh_routes/relay.rs

#### CRITICAL

**`relay.rs:89–97` — `path_requires_full_access` allowlist is incomplete; relay can reach pairing and introduction endpoints.**
The function gates only 3 paths. Any other path — including `/api/mesh/pair/request`, `/api/mesh/pair/accept`, `/api/mesh/introduce`, `/api/agents/spawn`, `/api/panes/create`, and every other local API endpoint — is reachable via relay by any paired peer with `AccessLevel::Full`. A compromised `Full`-access peer can relay `POST /api/mesh/pair/request` with an attacker-controlled `PairingRequest` body (arbitrary `from_instance_id`, attacker's `public_key`), injecting a fake pairing request into the local node's `pending_requests` queue without any out-of-band pairing UI interaction. User approval is still required to accept, so the final accept step requires user action — but the injection itself is silent and the UI will show an unsolicited incoming request from any claimed identity.

**`relay.rs:162` — `cmd.path` is passed directly into `local_url` with no validation.**
```rust
let local_url = format!("http://127.0.0.1:{}{}", state.api_port, cmd.path);
```
There is no check that `cmd.path` starts with `/api/`. A peer could supply `cmd.path = "/../../../etc/passwd"` (path traversal against local HTTP), `cmd.path = ""` (hits root), or any non-API route. In practice, axum will 404 on unrecognized paths, but the door is open.

**`relay.rs:131` — `access_level` defaults to `Full` when peer not found after auth.**
```rust
.unwrap_or(crate::mesh::types::AccessLevel::Full)
```
`check_mesh_relay_auth` verified the peer is paired before reaching this code, but there is a small TOCTOU window: if another thread unpairs the peer between auth check and access check, the access lookup returns `None`, which resolves to `Full`, bypassing any `ReadOnly`/`AgentIsolated` restriction. Should default to `ReadOnly` or return an error.

**`relay.rs:67` — `.unwrap()` on `serde_json::to_value(our_payload)` in `mesh_sync`.**
Same class as mod.rs CRITICAL above.

#### HIGH

**`relay.rs:166–180` — Relay re-signs with this node's own identity token and forwards to itself.**
The relay authenticates the incoming peer, then signs a new local token and submits the request to `http://127.0.0.1:{port}{path}`. This means the relayed request arrives at the local API as "from self" — it has the local node's full auth identity. This is by design (the relay acts on behalf of the peer) but it means access_level enforcement on the relay endpoint is the ONLY guard. Any gap in `path_requires_full_access` (see CRITICAL above) allows a paired peer to perform any local operation with local authority.

---

### src-tauri/src/api/mesh_routes/pairing.rs

#### HIGH

**`pairing.rs:23–85` — `mesh_pair_accept` is unauthenticated.**
`mesh_pair_accept` receives a `PairingAccept` body with no auth check. Any host that knows a valid `request_id` (which is a UUID4, so not guessable) and any `from_instance_id` + `public_key` can complete a pairing on behalf of the real peer. The `request_id` is sent over unencrypted HTTP to the remote peer, so an on-path observer could intercept it and race to send a forged `PairingAccept` with their own `public_key`, completing the pairing with the wrong keypair. This is the MITM-in-pairing risk flagged in `project_mesh_security_gaps.md`.

**`pairing.rs` — `mesh_pair_request` is intentionally unauthenticated, but there is no rate limiting.**
Any peer on the network that knows the API port can flood `POST /api/mesh/pair/request` with unique `request_id`s. `receive_pairing_request` limits concurrent pending requests to 5 per-instance, but does not limit request rate. A flooding peer can keep the pending queue saturated (5 entries × constant churn), preventing legitimate pairing requests from being queued.

#### MEDIUM

**`pairing.rs:55` — `pairing_targets.remove` only on successful accept; orphaned entries on failure.**
If the requester sends `pair/request` and the approver rejects or never responds, `pairing_targets` retains the entry. `pending_requests` has `cleanup_expired_requests` (called by `mesh_list_peers`), but `pairing_targets` has no corresponding cleanup. The two maps can diverge.

---

### src-tauri/src/api/mesh_routes/connection.rs

#### HIGH

**`connection.rs:78–152` — `mesh_approve_peer` (REST) missing `broadcast_introductions`.**
`mesh_approve_pairing` (Tauri, `commands_mesh.rs:149–151`) calls `broadcast_introductions` to auto-pair the new peer with all existing peers. `mesh_approve_peer` (REST `connection.rs`) calls `broadcast_announce + broadcast_sync` but NOT `broadcast_introductions`. The two code paths implement the same logical action (approve an incoming pairing request) but produce different mesh topology outcomes. When a user approves via the REST/CLI path, existing peers are never told about the new peer via the introduction mechanism; they only learn via gossip sync (which adds them as `discovered`, not `paired`).

**`connection.rs:247` — `mesh_disconnect_peer` cascade uses `addresses.first()` not address-tier selection.**
```rust
// In the for-peer cascade:
// uses try_peer which sorts by tier — OK
```
Actually, the cascade does use `try_peer` (line 266). BUT `mesh_exit` in `commands_mesh.rs:277` uses `.addresses.first()` directly instead of `address_tier`:
```rust
.map(|p| (p.addresses.first().map(|a| a.ip.clone()), p.api_port, our_id.clone()))
```
This means `mesh_exit` prefers whichever address happens to be first in the list (often LAN, not Tailscale), while all other outbound calls use the tier-ranked selection. Inconsistent.

#### MEDIUM

**`connection.rs` and `commands_mesh.rs` — Pairing initiation logic is duplicated verbatim.**
`mesh_connect_peer` (connection.rs:13–75) and `mesh_connect` (commands_mesh.rs:32–88) are line-for-line duplicates: same `PairingRequest` construction, same `pairing_targets.insert`, same HTTP send. Any bug fix in one is not automatically applied to the other. The business logic belongs in a single function in `mesh/` that both callers invoke.

Same duplication exists for `mesh_approve_peer` / `mesh_approve_pairing` (modulo the missing `broadcast_introductions` difference noted above) and `mesh_reject_peer` / `mesh_reject_pairing`.

---

### src-tauri/src/api/mesh_routes/introduction.rs

#### HIGH

**`introduction.rs:10–95` — Introduction unconditionally pairs at `AccessLevel::Full`; no consent.**
When `mesh_introduce` receives a `PeerIntroduction` from a paired peer, it immediately adds the introduced peer as a `PairedPeer` with `access_level: Full` and saves to disk. There is no user confirmation step. A compromised or malicious paired peer can force-pair any instance_id/public_key combination onto the local node simply by calling `/api/mesh/introduce`. The "sender must be paired" check prevents a random external host from doing this, but any single trusted peer can introduce an unlimited number of arbitrary peers.

**`introduction.rs:86–94` — Response leaks local IP in plaintext.**
```rust
Ok(Json(serde_json::json!({
    "ok": true,
    "instance_id": ...,
    "public_key": ...,
    "local_ip": mesh.local_ip,  // ← leaked
})))
```
The response returns `local_ip` to the sender. Since the sender is already a paired peer this is low risk, but it is not needed by the caller (`broadcast_introductions` does not use the response body at all).

---

### src-tauri/src/api/mesh_routes/announce.rs

#### OK

Inbound announce correctly verifies sender identity via `verify_signed_request`, enforces sender-ID/body-ID match via `apply_announce`, and sanitizes addresses. No panicking unwraps.

---

### src-tauri/src/commands_mesh.rs

#### HIGH

**`commands_mesh.rs:267–300` — `mesh_exit` uses `.addresses.first()` instead of address-tier selection.**
All outbound calls to peers use the address-tier ranking (Tailscale preferred), but `mesh_exit` builds its peer list using `.addresses.first()`. If the first address in the persisted list is LAN (reachable only on the same subnet) and the actual reachable address is Tailscale, the exit notification silently fails. Peers are not notified and their local state retains the exiting node as paired indefinitely.

#### MEDIUM

**`commands_mesh.rs:151` — `broadcast_introductions` is awaited inline, blocking the Tauri command response.**
`mesh_approve_pairing` calls `broadcast_introductions(...).await` directly (not in a `tokio::spawn`). In a large mesh, this fans out to N × 2 HTTP calls (one per pair direction per existing peer) before the command returns. The Tauri frontend is blocked waiting for the command result for the entire duration. Compare with the REST path which wraps broadcasts in `tokio::spawn` with a short delay. Should be spawned.

**`commands_mesh.rs:32–88` and `connection.rs:13–75` — Duplicated pairing initiation.** (see connection.rs MEDIUM above)

#### LOW

**`commands_mesh.rs` — `mesh_pair_confirm` is a no-op stub.**
The Tauri command does not exist, but the REST route `mesh_pair_confirm` (pairing.rs:88–93) is a stub that always returns `{"ok": true}` without doing anything. If this is a deprecated step, it should be removed. If it is intended for a future protocol version, it should be marked as such.

---

## Summary Table

| Severity | Count |
|----------|-------|
| CRITICAL | 4 |
| HIGH | 10 |
| MEDIUM | 10 |
| LOW | 4 |
| **Total** | **28** |

---

## Critical Finding Summary

1. **Replay attacks not prevented** — nonces are generated and signed but never recorded; any intercepted signed token works for up to 300 s on any authenticated endpoint.
2. **Relay endpoint lacks path allowlist** — a paired `Full`-access peer can relay to `/api/mesh/pair/request`, `/api/mesh/introduce`, `/api/agents/spawn`, and every other local API endpoint; only 3 specific paths are gated.
3. **`mesh_pair_accept` is unauthenticated** — any host that can guess or observe the `request_id` UUID can complete pairing with a forged public key (MITM in pairing).
4. **Introduction auto-pairs without user consent** — any single trusted paired peer can force-pair arbitrary instances at `Full` access by calling `/api/mesh/introduce`.

<!-- revision: 1 -->
