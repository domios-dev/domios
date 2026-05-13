# Security Hardening — All CRITICAL Findings

**Date:** 2026-05-13
**Source:** Full project code review (docs/superpowers/review/2026-05-13/consolidated.md)
**Total CRITICAL findings addressed:** 18

## Decomposition into parallel groups

Groups A–D touch non-overlapping files and can be executed in parallel worktrees.

---

## Group A — Quick Safety Fixes

**Files:**
- `src-tauri/src/commands/completions.rs`
- `src-tauri/src/startup/window.rs`
- `cli/src/worker.rs` (line 452 only)
- `cli/src/main.rs` (line 144 only)
- `src-tauri/src/api/notification_routes.rs`
- `src-tauri/src/api/hook_routes.rs`

### A1 — completions.rs:13: explicit panic!() in Tauri command

**Problem:** `entry.file_type().unwrap_or_else(|_| panic!())` crashes the entire daemon on a broken symlink or permission error.

**Fix:** Replace with a graceful skip:
```rust
// before
entry.file_type().unwrap_or_else(|_| panic!())

// after  
let Ok(ft) = entry.file_type() else { continue; };
```

### A2 — window.rs:21: unwrap() on native window handle

**Problem:** `window.ns_window().unwrap()` panics at startup if Tauri returns None for the native handle.

**Fix:**
```rust
// before
let ns_win = window.ns_window().unwrap();

// after
let Some(ns_win) = window.ns_window() else { return; };
```

### A3 — worker.rs:452: panic on non-ASCII skill names

**Problem:** `&trimmed[..40]` slices at byte offset 40, panicking on any non-ASCII character (Chinese, emoji, etc.) in a skill name.

**Fix:** Use `char_indices` to slice at char boundary:
```rust
// before
&trimmed[..40]

// after
trimmed.char_indices().nth(40).map_or(trimmed, |(i, _)| &trimmed[..i])
```

### A4 — main.rs:144: HTTP client fallback drops auth token

**Problem:** `build_http()` builder failure falls back to `Client::new()` via `unwrap_or_default()`, dropping all default headers including the auth token. Every subsequent request silently returns 401.

**Fix:** Propagate the error instead of silently falling back:
```rust
// before
let client = build_http(...).unwrap_or_default();

// after
let client = build_http(...)?;
```
(Adjust error handling at call site as appropriate for the function's return type.)

### A5 — notification_routes.rs:62: serve_file without auth (arbitrary filesystem read)

**Problem:** `serve_file` serves arbitrary files from disk to any caller — no authentication, no path restriction, no traversal guard.

**Fix:**
1. Add `check_auth` call at the start of the handler.
2. Add a path allowlist: only serve from within `~/.codeterm/` (dev: `~/.codeterm-dev/`). Reject any path that doesn't start with the expected prefix after canonicalization.
3. If the endpoint only serves a specific known file, change signature to accept no path parameter and hardcode the path.

```rust
pub async fn serve_file(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(params): Query<ServeFileParams>,
) -> Result<..., (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    
    let requested = std::path::Path::new(&params.path).canonicalize()
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid path".to_string()))?;
    let allowed_prefix = codeterm_data_dir(); // returns ~/.codeterm[-dev]/
    if !requested.starts_with(&allowed_prefix) {
        return Err((StatusCode::FORBIDDEN, "Path not allowed".to_string()));
    }
    // ... rest of handler
}
```

### A6 — hook_routes.rs:63,141,178: three unauthenticated hook endpoints

**Problem:** `hook_permission`, `hook_stop`, and `hook_session_start` have no auth check. Any process reaching the port can inject session IDs, clear agent state, and trigger permission-approval flows.

**Fix:** Add `check_auth` as the first call in all three handlers:
```rust
pub async fn hook_permission(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    // ... rest of params
) -> Result<...> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    // ... existing logic
}
```
Apply same pattern to `hook_stop` and `hook_session_start`.

---

## Group B — Mesh Security

**Files:**
- `src-tauri/src/mesh/crown.rs` — nonce tracking
- `src-tauri/src/mesh/mod.rs` — add `SeenNonces` to `MeshState`
- `src-tauri/src/api/mesh_routes/pairing.rs` — pair_accept auth
- `src-tauri/src/api/mesh_routes/relay.rs` — expand denylist
- `src-tauri/src/api/mesh_routes/introduction.rs` — consent (pending queue)

### B1 — Replay attack prevention (crown.rs + mesh/mod.rs)

**Problem:** Nonces are generated and included in `SignedRequest` but never recorded. Any intercepted signed token is fully replayable for 300 seconds.

**Fix:**
1. Add to `MeshState` (mesh/mod.rs):
```rust
pub seen_nonces: std::collections::HashMap<String, u64>, // nonce → timestamp
```

2. In `verify_signed_request` (crown.rs), add seen-nonce check after timestamp validation. Caller passes `seen_nonces` mutably:
```rust
pub fn verify_signed_request(
    token: &str,
    known_keys: &HashMap<String, String>,
    seen_nonces: &mut HashMap<String, u64>,
) -> Result<String, String> {
    // ... existing timestamp check ...
    
    // Replay check
    if seen_nonces.contains_key(&req.nonce) {
        return Err("Replay detected: nonce already seen".to_string());
    }
    
    // ... existing signature verification ...
    
    // Record nonce after successful verify
    seen_nonces.insert(req.nonce.clone(), req.timestamp);
    
    // Purge expired nonces (older than 310s — slightly past the 300s window)
    seen_nonces.retain(|_, &mut ts| now.saturating_sub(ts) <= 310);
    
    Ok(req.from_instance_id)
}
```

3. Update all call sites of `verify_signed_request` to pass `&mut mesh.seen_nonces`. Sites: `relay.rs::check_mesh_relay_auth`, `announce.rs`, `connection.rs`, `introduction.rs`, `pairing.rs::mesh_unpair`, `relay.rs::sender_id_from_headers`.

4. `seen_nonces` is not persisted (in-memory only). On restart, nonces reset — acceptable since 300s window is short.

### B2 — mesh_pair_accept: validate sender identity (pairing.rs:21)

**Problem:** Any caller knowing the `request_id` UUID can complete the pairing with their own `public_key`. The UUID is sent over plain HTTP.

**Root cause:** `pairing_targets` stores `(address, port, instance_id)` but `instance_id` is not validated against `accept.from_instance_id`.

**Fix:** Validate that `accept.from_instance_id` matches what was stored in `pairing_targets`:
```rust
pub async fn mesh_pair_accept(...) -> Result<...> {
    let (b_address, b_port, b_instance_id) = {
        let mesh = state.mesh.lock();
        mesh.pairing_targets
            .get(&accept.request_id)
            .cloned()
            .ok_or_else(|| (StatusCode::NOT_FOUND, "No pending request".to_string()))?
    };
    
    // Verify the accepting peer is who we sent the request to
    if accept.from_instance_id != b_instance_id {
        return Err((StatusCode::FORBIDDEN,
            format!("Identity mismatch: expected {}, got {}", b_instance_id, accept.from_instance_id)));
    }
    
    // ... rest of existing logic unchanged
}
```

This requires that `pairing_targets` already stores `instance_id` alongside address/port — verify this in the code and add it to the `receive_pairing_request` path if missing.

### B3 — Relay denylist: protect sensitive paths (relay.rs:89)

**Problem:** `path_requires_full_access` only gates 3 paths. Paired peers can relay to `/api/agents/spawn`, `/api/mesh/pair/request`, `/api/mesh/introduce`, and all hook/pane/config endpoints.

**Fix:** Expand the gated list with all state-mutating dangerous paths:
```rust
fn path_requires_full_access(method: &str, path: &str) -> bool {
    let m = method.to_ascii_uppercase();
    let bare = path.split('?').next().unwrap_or(path);
    matches!(
        (m.as_str(), bare),
        // Existing gates
        ("POST", "/api/update/install")
            | ("POST", "/api/update/channel")
            | ("POST", "/api/mesh/refresh")
        // New gates: mesh state mutation
            | ("POST", "/api/mesh/pair/request")
            | ("POST", "/api/mesh/pair/accept")
            | ("POST", "/api/mesh/introduce")
            | ("DELETE", _) if bare.starts_with("/api/mesh/peers/")
        // Agent lifecycle
            | ("POST", "/api/agents/spawn")
        // Hook injection
            | ("POST", "/api/hooks/permission")
            | ("POST", "/api/hooks/stop")
            | ("POST", "/api/hooks/session_start")
        // Config mutation
            | ("POST", "/api/config")
            | ("PUT", "/api/config")
    )
}
```
Note: the DELETE pattern needs special handling since `matches!` can't use `starts_with`. Use a separate check or `||` chain.

### B4 — Introduction consent: pending queue instead of auto-pair (introduction.rs:56-73)

**Problem:** Any trusted peer can call `/api/mesh/introduce` and force-pair arbitrary instances at `AccessLevel::Full` with no user confirmation.

**Fix:** Instead of immediately pairing, add to `pending_requests` (reuses existing approval UI):
```rust
// After auth verification and blocked check...

log::info!("[mesh/introduce] {} introduced by {} — adding to pending (consent required)", 
    intro.instance_name, sender_id);

let request = crate::mesh::types::PendingPairingRequest {
    request_id: uuid::Uuid::new_v4().to_string(),
    from_instance_id: intro.instance_id.clone(),
    from_instance_name: intro.instance_name.clone(),
    from_address: intro.addresses.first().map(|a| a.ip.clone()).unwrap_or_default(),
    from_port: intro.api_port,
    status: crate::mesh::types::PairingStatus::Pending,
    received_at: chrono::Utc::now().to_rfc3339(),
    introduced_by: Some(sender_id.clone()),  // new field — add to PendingPairingRequest
};

let mut mesh = state.mesh.lock();
mesh.pending_requests.insert(request.request_id.clone(), request);
let _ = mesh.save();
drop(mesh);

let _ = state.svc.app_handle.emit("mesh-state-changed", ());
Ok(Json(serde_json::json!({"ok": true, "pending": true})))
```

Also:
- Add `introduced_by: Option<String>` field to `PendingPairingRequest` struct in `mesh/types.rs`
- Update the pairing UI to show "Introduced by X" label when `introduced_by` is Some

---

## Group C — Concurrency & Data Integrity

**Files:**
- `src-tauri/src/service/pty_lifecycle.rs` — lock ordering fix
- `src-tauri/src/service/spawn.rs` — lock ordering fix
- `src-tauri/src/service/orchestrator.rs` — orphaned pane cleanup
- `src-tauri/src/mem/graph.rs` — add_alias transaction
- `crates/mem-store/src/graph.rs` — add_alias_with_rewire + canonicalize_relation_types transactions

### C1 — Lock ordering: pty_lifecycle.rs::handle_pty_death (lines 166-167)

**Problem:** Acquires `agent_tracker` (position 4) before `mux` (position 1). Mandated order: mux → output_buf → screen_state → agent_tracker → pty.

**Fix:** Read the exact lines around 166-167 in `pty_lifecycle.rs` to confirm the inversion, then reorder acquisition so `mux` is taken first. If `agent_tracker` is needed for data and `mux` is needed for cleanup, extract the data from `agent_tracker` first, drop it, then acquire `mux`.

Pattern:
```rust
// WRONG: agent_tracker before mux
let tracker = state.agent_tracker.lock();
let mux = state.mux.lock();

// CORRECT: mux first
let mux = state.mux.lock();
// ... use mux ...
drop(mux);
let tracker = state.agent_tracker.lock();
// ... use tracker ...
```

### C2 — Lock ordering: spawn.rs::spawn_hidden_agent_impl

**Problem:** Same lock ordering inversion propagates through the outer→inner→reader→handle_pty_death call chain.

**Fix:** Same pattern as C1. Read spawn.rs to find the exact call path. The fix in C1 (handle_pty_death) may be sufficient if spawn.rs itself doesn't independently invert; verify after C1 fix.

### C3 — Orphaned pane on partial spawn failure (orchestrator.rs)

**Problem:** Tab+pane is created and partially mutated before `set_pane_role` runs. If role assignment fails, the pane is leaked in the mux with no cleanup path.

**Fix:** Wrap spawn sequence in a cleanup guard. If any step after pane creation fails, remove the pane from mux:
```rust
let pane_id = mux.create_pane(...)?;
let cleanup = scopeguard::defer(|| {
    // runs if we return Err below
    if let Ok(mut m) = state.mux.try_lock() {
        m.remove_pane(&pane_id);
    }
});
// ... setup steps ...
set_pane_role(...)?;
std::mem::forget(cleanup); // success path: cancel cleanup
```
Alternatively use a `Result`-based rollback pattern without `scopeguard` crate if it's not already a dependency.

### C4 — mem/graph.rs::add_alias: silent data loss without transaction

**Problem:** `UPDATE OR IGNORE` followed by `DELETE ... WHERE from_id = ?1 OR to_id = ?1` runs unconditionally. Relations that failed the IGNORE (unique conflict) are deleted without being moved.

**Fix:** Wrap the multi-step alias operation in a transaction:
```rust
let tx = conn.transaction()?;
// ... all UPDATE / INSERT / DELETE steps on tx ...
tx.commit()?;
```

### C5 — crates/mem-store/graph.rs: add_alias_with_rewire without transaction (line 384)

**Problem:** 5 destructive SQL statements with no transaction. Any failure at step 2–5 leaves the graph permanently inconsistent.

**Fix:** Same as C4 — wrap in `conn.transaction()?.commit()`.

### C6 — crates/mem-store/graph.rs: canonicalize_relation_types without transaction (line 280)

**Problem:** No transaction per synonym pair or across the loop. Partial failures leave both canonical and synonym relation types active on the same edges.

**Fix:** Wrap the synonym loop body in a savepoint or wrap the entire function call in a transaction.

---

## Group D — Architecture Fixes

**Files:**
- `src-tauri/src/mem/dreaming.rs` — split to shrink below 1095 (allowlist cap)
- `cli/src/cmd/bridge/migrate.rs` — atomic migration

### D1 — dreaming.rs: extract to bring under allowlist cap

**Problem:** dreaming.rs is 1096 lines — 1 past its 1095-line allowlist cap. CI fails.

**Fix:** Extract one coherent section to a new file. The natural split:
- Extract cron scheduling helpers (`ensure_dreaming_cron`, related fn) → `src-tauri/src/mem/dreaming/scheduling.rs`
- Keep core dreaming logic in `dreaming.rs`
- The result: `dreaming.rs` shrinks by ~50-100 lines, falling under 1095.
- Update `allowlist.txt` to remove the entry once `dreaming.rs` is under limit.

### D2 — cli/bridge/migrate.rs: atomic migration

**Problem:** Two-phase migration has no atomic lock. Config-disable + poller-stop are two separate steps with no recovery if process is killed between them.

**Fix:**
1. Write a `.migrate_lock` file before starting (to detect interrupted migrations on restart)
2. On startup: if `.migrate_lock` exists, re-run from step 1 (idempotent re-entry)
3. Remove `.migrate_lock` only after both steps complete successfully
4. Alternatively: use a single atomic operation (rename) for the config change, making it effectively crash-safe

---

## Success Criteria

Each group is done when:
- All CRITICAL findings in the group are fixed
- `cargo check` passes (no new compiler errors)
- `./build.sh test` passes (existing tests still green)
- No new `unwrap()` in non-test production code introduced by the fix
- Group A: `check_auth` present on all 5 affected endpoints
- Group B: replay test passes (same token rejected on second call)
- Group C: lock ordering matches mux→output_buf→screen_state→agent_tracker→pty in all modified paths
- Group D: `scripts/check-file-sizes.sh` passes (dreaming.rs under cap)
