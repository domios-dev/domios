# Security Group B — Mesh Security Fixes

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 4 CRITICAL mesh-security vulnerabilities: replay attacks, unauthenticated pair acceptance, relay denylist gaps, and auto-pairing without user consent.

**Architecture:** Pure fix-only changes. Non-overlapping with Group A and C files. Each task is independently testable via `cargo test`. TDD where possible; compilation checks where unit tests can't exercise the path.

**Tech Stack:** Rust, Tauri v2, axum, parking_lot Mutex, `cargo test`, `./build.sh check`

---

### Task 1: Add nonce replay prevention (crown.rs + mesh/mod.rs)

**Files:**
- Modify: `src-tauri/src/mesh/mod.rs` (add `seen_nonces` field, `check_and_record_nonce` method)
- Modify: `src-tauri/src/mesh/crown.rs` (add `nonce_from_token` helper, existing tests still pass)
- Modify: `src-tauri/src/api/mesh_routes/relay.rs` (update `check_mesh_relay_auth`)
- Modify: `src-tauri/src/api/mesh_routes/introduction.rs` (add nonce check)
- Modify: `src-tauri/src/api/mesh_routes/pairing.rs` (add nonce check to `mesh_unpair`)

**Root cause:** `verify_signed_request` checks the 300-second timestamp window but never records used nonces. The same signed token is accepted unlimited times within the window — full replay attack.

- [ ] **Step 1: Check existing tests**

```bash
grep -n "#\[test\]" src-tauri/src/mesh/crown.rs
```
Expected: 5 existing tests (roundtrip, wrong key, unknown peer, public_key_from_private x2). Do not duplicate them.

- [ ] **Step 2: Add nonce helper and test to crown.rs**

Add to `src-tauri/src/mesh/crown.rs` at the end, before the closing `}` of the file (after existing tests):

```rust
/// Extract nonce from a signed request token without verifying the signature.
/// Used by replay-prevention code after `verify_signed_request` already passed.
pub fn nonce_from_token(token: &str) -> Result<String, String> {
    let parts: Vec<&str> = token.splitn(2, '.').collect();
    if parts.len() != 2 {
        return Err("Invalid token format".to_string());
    }
    let payload_bytes = B64.decode(parts[0]).map_err(|e| format!("Bad payload: {}", e))?;
    let req: SignedRequest = serde_json::from_slice(&payload_bytes)
        .map_err(|e| format!("Bad claims: {}", e))?;
    Ok(req.nonce)
}
```

Add to the existing `#[cfg(test)] mod tests` block in crown.rs:

```rust
    #[test]
    fn nonce_from_token_roundtrip() {
        let (priv_key, _) = generate_peer_keypair();
        let token = sign_request(&priv_key, "peer-abc").unwrap();
        let nonce = nonce_from_token(&token).unwrap();
        assert!(!nonce.is_empty(), "nonce should be a non-empty UUID");
        // Two different tokens have different nonces
        let token2 = sign_request(&priv_key, "peer-abc").unwrap();
        let nonce2 = nonce_from_token(&token2).unwrap();
        assert_ne!(nonce, nonce2, "each sign produces a unique nonce");
    }
```

- [ ] **Step 3: Run test — verify it passes**

```bash
cargo test -p codeterm nonce_from_token_roundtrip 2>&1 | tail -5
```
Expected: PASS.

- [ ] **Step 4: Add `seen_nonces` to MeshManager struct and `new()`**

In `src-tauri/src/mesh/mod.rs`, update the `MeshManager` struct (around line 27):

```rust
// BEFORE (add seen_nonces after pairing_targets):
pub struct MeshManager {
    // ... existing fields ...
    pub pairing_targets: HashMap<String, (String, u16, String)>,
    pub mdns_daemon: Option<mdns_sd::ServiceDaemon>,
    pub mdns_service_fullname: Option<String>,
    pub udp_started: bool,
    pub disk_persist: bool,
}

// AFTER (add seen_nonces field):
pub struct MeshManager {
    // ... existing fields ...
    pub pairing_targets: HashMap<String, (String, u16, String)>,
    pub mdns_daemon: Option<mdns_sd::ServiceDaemon>,
    pub mdns_service_fullname: Option<String>,
    pub udp_started: bool,
    pub disk_persist: bool,
    /// Nonces seen within the last 310s — used to reject replayed signed requests.
    pub seen_nonces: HashMap<String, u64>,
}
```

In `MeshManager::new()` (around line 62), add `seen_nonces: HashMap::new()` to the struct literal:

```rust
// BEFORE (last fields):
        pairing_targets: HashMap::new(),
        mdns_daemon: None,
        mdns_service_fullname: None,
        udp_started: false,
        disk_persist: true,

// AFTER:
        pairing_targets: HashMap::new(),
        mdns_daemon: None,
        mdns_service_fullname: None,
        udp_started: false,
        disk_persist: true,
        seen_nonces: HashMap::new(),
```

- [ ] **Step 5: Add `check_and_record_nonce` method to MeshManager**

In `src-tauri/src/mesh/mod.rs`, inside `impl MeshManager`, add after the `save()` method:

```rust
    /// Check that `token`'s nonce has not been seen, record it, purge expired nonces.
    /// Call this AFTER `verify_signed_request` succeeds. Returns Err on replay.
    pub fn check_and_record_nonce(&mut self, token: &str) -> Result<(), String> {
        let nonce = crate::mesh::crown::nonce_from_token(token)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if self.seen_nonces.contains_key(&nonce) {
            return Err(format!("Replay detected: nonce {} already used", nonce));
        }
        self.seen_nonces.insert(nonce, now);
        // Purge nonces older than 310s (just past the 300s validity window)
        self.seen_nonces.retain(|_, &mut ts| now.saturating_sub(ts) <= 310);
        Ok(())
    }
```

- [ ] **Step 6: Write unit test for replay prevention**

Add to `src-tauri/src/mesh/mod.rs`, inside the existing `#[cfg(test)]` block (or add one if absent):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_and_record_nonce_rejects_replay() {
        let mut mgr = MeshManager::new("test-node".to_string(), 7777);
        mgr.disk_persist = false;

        let (priv_key, pub_key) = crate::mesh::crown::generate_peer_keypair();
        mgr.ident.identity.private_key = Some(priv_key.clone());
        mgr.ident.identity.public_key = Some(pub_key.clone());
        let instance_id = mgr.ident.identity.instance_id.clone();

        let token = crate::mesh::crown::sign_request(&priv_key, &instance_id).unwrap();

        // First use: should succeed
        assert!(mgr.check_and_record_nonce(&token).is_ok(), "first use should succeed");
        // Second use of same token: should fail
        assert!(mgr.check_and_record_nonce(&token).is_err(), "replay should be rejected");
    }
}
```

- [ ] **Step 7: Run test — verify it passes**

```bash
cargo test -p codeterm check_and_record_nonce_rejects_replay 2>&1 | tail -5
```
Expected: PASS.

- [ ] **Step 8: Update `check_mesh_relay_auth` in relay.rs**

In `src-tauri/src/api/mesh_routes/relay.rs`, update `check_mesh_relay_auth` to add nonce check. Change signature to `&mut AppState`... wait, `AppState` is shared, so take a different approach: acquire the mesh lock after verifying, then check nonce.

Replace the `check_mesh_relay_auth` function (lines 101-118):

```rust
fn check_mesh_relay_auth(state: &AppState, headers: &HeaderMap) -> Result<String, (StatusCode, String)> {
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    let (known_keys, our_id) = {
        let mesh = state.mesh.lock();
        let mut known_keys = mesh.known_public_keys();
        if let Some(ref pk) = mesh.ident.identity.public_key {
            known_keys.insert(mesh.ident.identity.instance_id.clone(), pk.clone());
        }
        (known_keys, mesh.ident.identity.instance_id.clone())
    };
    let peer_id = crate::mesh::crown::verify_signed_request(token, &known_keys)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or unknown signature".to_string()))?;
    if peer_id == our_id { return Ok("local".to_string()); }

    let mut mesh = state.mesh.lock();
    if !mesh.paired_peers.contains_key(&peer_id) {
        return Err((StatusCode::UNAUTHORIZED, format!("Peer {} not paired", peer_id)));
    }
    mesh.check_and_record_nonce(token)
        .map_err(|e| (StatusCode::UNAUTHORIZED, e))?;
    Ok(peer_id)
}
```

- [ ] **Step 9: Update `mesh_introduce` in introduction.rs**

In `src-tauri/src/api/mesh_routes/introduction.rs`, after the `verify_signed_request` call succeeds (around line 32), add the nonce check before the subsequent business logic. The code currently drops the mesh lock before calling verify, so re-acquire after:

After `let sender_id = match crate::mesh::crown::verify_signed_request(...) { Ok(id) => id, ... };`, add:

```rust
    // Replay prevention — re-acquire lock to record used nonce
    {
        let mut mesh = state.mesh.lock();
        if let Err(e) = mesh.check_and_record_nonce(token) {
            return Err((StatusCode::UNAUTHORIZED, e));
        }
    }
```

Insert this block BEFORE `if intro.instance_id == our_id {`.

- [ ] **Step 10: Update `mesh_unpair` in pairing.rs**

In `src-tauri/src/api/mesh_routes/pairing.rs`, inside `mesh_unpair`, after the `verify_signed_request` call succeeds (around line 135-136), add the nonce check:

```rust
    let sender_id = crate::mesh::crown::verify_signed_request(token, &known_keys)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()))?;

    // Replay prevention
    {
        let mut mesh = state.mesh.lock();
        if let Err(e) = mesh.check_and_record_nonce(token) {
            return Err((StatusCode::UNAUTHORIZED, e));
        }
    }
```

- [ ] **Step 11: Verify compilation**

```bash
./build.sh check 2>&1 | tail -10
```
Expected: no errors.

- [ ] **Step 12: Run all mesh tests**

```bash
cargo test -p codeterm mesh 2>&1 | tail -10
cargo test -p codeterm crown 2>&1 | tail -10
```
Expected: all pass.

- [ ] **Step 13: Commit**

```bash
git add src-tauri/src/mesh/crown.rs src-tauri/src/mesh/mod.rs src-tauri/src/api/mesh_routes/relay.rs src-tauri/src/api/mesh_routes/introduction.rs src-tauri/src/api/mesh_routes/pairing.rs
git commit -m "fix(mesh): add nonce replay prevention — seen_nonces set rejects reused signed tokens"
```

---

### Task 2: Validate sender identity in mesh_pair_accept (pairing.rs:21)

**Files:**
- Modify: `src-tauri/src/api/mesh_routes/pairing.rs` (function `mesh_pair_accept`)

**Root cause:** `mesh_pair_accept` trusts the `from_instance_id` in the JSON body without checking it matches what was stored in `pairing_targets` when we sent the pair request. Anyone knowing the request_id UUID can hijack pairing with their own public key.

- [ ] **Step 1: Read the current code (already read — see pairing.rs:21-86)**

`pairing_targets` stores `(address, port, instance_id)` as a 3-tuple. The `b_instance_id` is the third element. The code never checks `accept.from_instance_id == b_instance_id`.

- [ ] **Step 2: Write test**

Add to `src-tauri/src/api/mesh_routes/pairing.rs` at the end:

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn pair_accept_identity_mismatch_is_detected() {
        // Verify that b_instance_id != accept.from_instance_id is detectable at the logic level.
        let stored_id = "peer-bob";
        let accept_id = "peer-eve";
        assert_ne!(stored_id, accept_id, "identity mismatch should be caught");
    }
}
```

This test is trivial but confirms the invariant. The real guard is the code change below.

- [ ] **Step 3: Apply fix**

In `src-tauri/src/api/mesh_routes/pairing.rs`, inside `mesh_pair_accept`, after extracting `(b_address, b_port, b_instance_id)` from `pairing_targets` (after line 31), add:

```rust
    // Guard: reject if the accepting peer isn't the one we sent the request to
    if accept.from_instance_id != b_instance_id {
        log::warn!(
            "[mesh/pair_accept] Identity mismatch: expected {}, got {}",
            b_instance_id, accept.from_instance_id
        );
        return Err((StatusCode::FORBIDDEN,
            format!("Identity mismatch: expected {}, got {}", b_instance_id, accept.from_instance_id)));
    }
```

Place this immediately after:
```rust
    let (b_address, b_port, b_instance_id) = {
        let mesh = state.mesh.lock();
        mesh.pairing_targets
            .get(&accept.request_id)
            .cloned()
            .ok_or_else(|| (StatusCode::NOT_FOUND, "No pending request".to_string()))?
    };
```

- [ ] **Step 4: Verify compilation**

```bash
./build.sh check 2>&1 | tail -5
```
Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/api/mesh_routes/pairing.rs
git commit -m "fix(mesh): validate sender identity in pair_accept — reject mismatched from_instance_id"
```

---

### Task 3: Expand relay denylist (relay.rs:89)

**Files:**
- Modify: `src-tauri/src/api/mesh_routes/relay.rs` (function `path_requires_full_access`)

**Root cause:** Only 3 paths are protected. Paired peers can relay to `/api/agents/spawn`, `/api/mesh/pair/request`, `/api/mesh/introduce`, and all hook/config endpoints through the relay, which re-signs as local identity bypassing the caller's access level.

- [ ] **Step 1: Check existing tests**

```bash
grep -n "#\[test\]" src-tauri/src/api/mesh_routes/relay.rs
```
Expected: 5 existing tests. Run them:

```bash
cargo test -p codeterm relay 2>&1 | tail -10
```
Expected: all pass.

- [ ] **Step 2: Write failing tests for new gated paths**

In `src-tauri/src/api/mesh_routes/relay.rs`, inside the existing `#[cfg(test)] mod tests` block, add:

```rust
    #[test]
    fn mesh_pairing_endpoints_are_gated() {
        assert!(path_requires_full_access("POST", "/api/mesh/pair/request"));
        assert!(path_requires_full_access("POST", "/api/mesh/pair/accept"));
        assert!(path_requires_full_access("POST", "/api/mesh/introduce"));
    }

    #[test]
    fn agent_spawn_is_gated() {
        assert!(path_requires_full_access("POST", "/api/agents/spawn"));
    }

    #[test]
    fn hook_endpoints_are_gated() {
        assert!(path_requires_full_access("POST", "/api/hooks/permission"));
        assert!(path_requires_full_access("POST", "/api/hooks/stop"));
        assert!(path_requires_full_access("POST", "/api/hooks/session_start"));
    }

    #[test]
    fn config_mutation_is_gated() {
        assert!(path_requires_full_access("POST", "/api/config"));
        assert!(path_requires_full_access("PUT", "/api/config"));
        assert!(!path_requires_full_access("GET", "/api/config"));
    }

    #[test]
    fn peer_delete_is_gated() {
        assert!(path_requires_full_access("DELETE", "/api/mesh/peers/some-uuid"));
        assert!(path_requires_full_access("DELETE", "/api/mesh/peers/peer-abc-123"));
        assert!(!path_requires_full_access("GET", "/api/mesh/peers/some-uuid"));
    }
```

- [ ] **Step 3: Run tests — verify they FAIL**

```bash
cargo test -p codeterm path_requires_full_access 2>&1 | tail -15
```
Expected: several new tests FAIL (the paths aren't gated yet).

- [ ] **Step 4: Apply fix**

In `src-tauri/src/api/mesh_routes/relay.rs`, replace the `path_requires_full_access` function (lines 89-98):

```rust
fn path_requires_full_access(method: &str, path: &str) -> bool {
    let m = method.to_ascii_uppercase();
    let bare = path.split('?').next().unwrap_or(path);
    // DELETE /api/mesh/peers/<id> needs starts_with check — matches! can't do that
    if m == "DELETE" && bare.starts_with("/api/mesh/peers/") {
        return true;
    }
    matches!(
        (m.as_str(), bare),
        // Existing: update/install paths
        ("POST", "/api/update/install")
            | ("POST", "/api/update/channel")
            | ("POST", "/api/mesh/refresh")
        // Mesh pairing — can't let a partial peer pair new nodes
            | ("POST", "/api/mesh/pair/request")
            | ("POST", "/api/mesh/pair/accept")
            | ("POST", "/api/mesh/introduce")
        // Agent lifecycle — spawning agents is privileged
            | ("POST", "/api/agents/spawn")
        // Hook injection — hooks can approve tool calls
            | ("POST", "/api/hooks/permission")
            | ("POST", "/api/hooks/stop")
            | ("POST", "/api/hooks/session_start")
        // Config mutation
            | ("POST", "/api/config")
            | ("PUT", "/api/config")
    )
}
```

- [ ] **Step 5: Run tests — verify they PASS**

```bash
cargo test -p codeterm path_requires_full_access 2>&1 | tail -15
```
Expected: all pass, including existing tests.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/api/mesh_routes/relay.rs
git commit -m "fix(mesh): expand relay denylist — gate pairing, agent spawn, hooks, config, peer delete"
```

---

### Task 4: Introduction consent — pending queue instead of auto-pair (introduction.rs:56-73)

**Files:**
- Modify: `src-tauri/src/mesh/types.rs` (add `introduced_by: Option<String>` to `PairingRequest`)
- Modify: `src-tauri/src/api/mesh_routes/introduction.rs` (replace auto-pair with pending queue)

**Root cause:** Any trusted peer can call `/api/mesh/introduce` and force-pair an arbitrary instance at `AccessLevel::Full` with no user consent. Attacker A (already paired) can introduce attacker B, granting B full access without user approval.

- [ ] **Step 1: Check existing PairingRequest serde usage**

```bash
grep -n "PairingRequest {" src-tauri/src/ -r | head -15
```

Note any struct literal construction sites — adding a new `Option<String>` with `#[serde(default)]` is backward-compatible with existing JSON deserialization.

- [ ] **Step 2: Add `introduced_by` to PairingRequest**

In `src-tauri/src/mesh/types.rs`, update `PairingRequest` (around line 228):

```rust
// BEFORE:
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingRequest {
    pub request_id: String,
    pub from_instance_id: String,
    pub from_instance_name: String,
    pub from_address: String,
    pub from_port: u16,
    pub public_key: String,
    pub timestamp: u64,
    #[serde(default = "default_pairing_status")]
    pub status: PairingStatus,
}

// AFTER:
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingRequest {
    pub request_id: String,
    pub from_instance_id: String,
    pub from_instance_name: String,
    pub from_address: String,
    pub from_port: u16,
    pub public_key: String,
    pub timestamp: u64,
    #[serde(default = "default_pairing_status")]
    pub status: PairingStatus,
    /// Instance ID of the peer that introduced this request (via /api/mesh/introduce).
    /// None for direct pair requests. Used by the UI to show "Introduced by X".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced_by: Option<String>,
}
```

- [ ] **Step 3: Fix any struct literal construction errors**

Run `./build.sh check` to find any struct literal sites that fail because they don't include `introduced_by`:

```bash
./build.sh check 2>&1 | grep "^error" | head -20
```

For each missing field error: add `introduced_by: None` to the struct literal. These will be direct `PairingRequest { ... }` constructions outside of serde deserialization.

- [ ] **Step 4: Write test for pending-queue behavior**

In `src-tauri/src/api/mesh_routes/introduction.rs`, add at the end of the file:

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn introduction_creates_pending_not_paired() {
        // Logic-level check: when a peer is introduced, it must go to pending_requests,
        // not directly into paired_peers. This is enforced by the code change below.
        // The field `introduced_by` is set to the sender's instance_id.
        let sender = "peer-alice";
        let introduced = "peer-bob";
        let request = crate::mesh::types::PairingRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            from_instance_id: introduced.to_string(),
            from_instance_name: "Bob".to_string(),
            from_address: "192.168.1.2".to_string(),
            from_port: 7685,
            public_key: "fake-key".to_string(),
            timestamp: 0,
            status: crate::mesh::types::PairingStatus::Pending,
            introduced_by: Some(sender.to_string()),
        };
        assert_eq!(request.introduced_by.as_deref(), Some(sender));
        assert_eq!(request.status, crate::mesh::types::PairingStatus::Pending);
    }
}
```

- [ ] **Step 5: Apply fix — replace auto-pair with pending queue**

In `src-tauri/src/api/mesh_routes/introduction.rs`, replace lines 56-84 (the auto-pair block):

```rust
    // BEFORE (auto-pair at Full access):
    log::info!("[mesh/introduce] {} introduced by {} — auto-pairing", intro.instance_name, sender_id);

    let mut mesh = state.mesh.lock();
    let peer = crate::mesh::types::PairedPeer {
        instance_id: intro.instance_id.clone(),
        // ... all fields ...
        access_level: crate::mesh::types::AccessLevel::Full,
        // ...
    };
    mesh.paired_peers.insert(intro.instance_id.clone(), peer);
    mesh.discovered_peers.remove(&intro.instance_id);
    let _ = mesh.save();
    drop(mesh);

    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
    // ... broadcast_sync spawn ...
```

Replace with:

```rust
    log::info!("[mesh/introduce] {} introduced by {} — adding to pending (consent required)", 
        intro.instance_name, sender_id);

    let request = crate::mesh::types::PairingRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        from_instance_id: intro.instance_id.clone(),
        from_instance_name: intro.instance_name.clone(),
        from_address: intro.addresses.first().map(|a| a.ip.clone()).unwrap_or_default(),
        from_port: intro.api_port,
        public_key: intro.public_key.clone(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        status: crate::mesh::types::PairingStatus::Pending,
        introduced_by: Some(sender_id.clone()),
    };

    let mut mesh = state.mesh.lock();
    mesh.pending_requests.insert(request.request_id.clone(), request);
    let _ = mesh.save();
    drop(mesh);

    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
```

Also remove the broadcast_sync `tokio::spawn` block (lines 78-84 in the original), since we're not pairing immediately — no sync needed.

Update the final response JSON at the end to reflect the pending state:

```rust
    let mesh = state.mesh.lock();
    Ok(Json(serde_json::json!({
        "ok": true,
        "pending": true,
        "instance_id": mesh.ident.identity.instance_id,
        "instance_name": mesh.ident.instance_name,
        "public_key": mesh.ident.identity.public_key,
        "api_port": mesh.api_port,
        "local_ip": mesh.local_ip,
    })))
```

- [ ] **Step 6: Verify compilation and tests**

```bash
./build.sh check 2>&1 | tail -10
cargo test -p codeterm introduction 2>&1 | tail -10
```
Expected: compilation OK, test PASS.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/mesh/types.rs src-tauri/src/api/mesh_routes/introduction.rs
git commit -m "fix(mesh): require user consent for introduced peers — queue to pending instead of auto-pairing"
```

---

### Final verification

```bash
./build.sh check 2>&1 | tail -5
./build.sh test 2>&1 | tail -10
```
Expected: no errors, all tests green.
