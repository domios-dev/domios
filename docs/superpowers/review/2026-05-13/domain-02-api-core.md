# Domain Review: API Core Routes

## Files Reviewed

- `src-tauri/src/api/mod.rs` (24 lines)
- `src-tauri/src/api/routes.rs` (238 lines)
- `src-tauri/src/api/auth.rs` (321 lines)
- `src-tauri/src/api/agent_routes.rs` (407 lines)
- `src-tauri/src/api/bridge_routes.rs` (279 lines)
- `src-tauri/src/api/config_routes.rs` (160 lines)
- `src-tauri/src/api/cron_routes.rs` (131 lines)
- `src-tauri/src/api/events.rs` (252 lines)
- `src-tauri/src/api/health_routes.rs` (28 lines)
- `src-tauri/src/api/hook_routes.rs` (208 lines)
- `src-tauri/src/api/logs_routes.rs` (63 lines)
- `src-tauri/src/api/network_routes.rs` (78 lines)
- `src-tauri/src/api/notification_routes.rs` (93 lines)
- `src-tauri/src/api/os_notify.rs` (167 lines)
- `src-tauri/src/api/pane_routes.rs` (723 lines)
- `src-tauri/src/api/services_routes.rs` (183 lines)
- `src-tauri/src/api/session_routes.rs` (222 lines)
- `src-tauri/src/api/theme_routes.rs` (133 lines)
- `src-tauri/src/api/updater_routes.rs` (232 lines)
- `src-tauri/src/api/usage_routes.rs` (120 lines)
- `src-tauri/src/api/webhook_routes.rs` (62 lines)
- `src-tauri/src/api/workspace_routes.rs` (134 lines)

---

## Findings

### src-tauri/src/api/agent_routes.rs

#### HIGH

- `agent_routes.rs:159` — `get_pane_status`: uses `serde_json::to_value(info).unwrap()` in production path. If `info` is not serializable this panics. Should be `.map_err(|e| ...)` instead.

- `agent_routes.rs:214` — `get_reports`: uses `serde_json::to_value(&reports).unwrap_or_default()`. Silently returns `null` JSON if serialization fails — the caller gets a 200 with empty body and no error indication.

- `agent_routes.rs:288-290` — `agent_restart_handler`: `retry_after_secs.to_string().parse().unwrap()` to build a `HeaderValue`. This panics if the string representation of the u64 somehow cannot round-trip through `HeaderValue::from_str`, which is practically impossible but still an `unwrap` in a hot path. Use `HeaderValue::from_str(...).unwrap_or_else(|_| HeaderValue::from_static("60"))`.

- `agent_routes.rs:327-336` — `clear_prompt` and `set_agent_type`: both handlers accept write operations but have **no `check_auth` call**. Any unauthenticated caller who can reach the server (e.g. via a local process or a misconfigured reverse proxy) can clear prompts or change the agent type on any pane.

#### MEDIUM

- `agent_routes.rs:343-348` — `set_agent_type` accepts any `agent_type` string from the body without validation. An empty string or an unknown provider string is silently accepted and stored on the pane.

- `agent_routes.rs:109-119` — `batch_commands`: the batch payload is dispatched without any size cap. A caller can submit an arbitrarily large `commands` array and starve the executor.

#### LOW

- `agent_routes.rs:101-107` — `BatchOp.method` is a free-form `String` with no validation or enumeration. An unknown method string will just fail inside `batch()`, but the route accepts it silently at the API boundary.

---

### src-tauri/src/api/auth.rs

#### HIGH

- `auth.rs:43` — `is_custom_auth`: the predicate `path.starts_with("/api/mesh/peers/") && !path.ends_with("/peers")` skips auth for **all** `/api/mesh/peers/<anything>` paths except the literal `/api/mesh/peers`. This means `/api/mesh/peers/some-id/addr` (a state-mutating POST) bypasses the normal token check and is only gated by the mesh-relay custom logic. If the mesh relay handler itself does not re-verify the sender independently, any unauthenticated request can mutate peer addresses.

- `auth.rs:40-44` — `is_custom_auth` is documented as "custom auth" but the corresponding handlers (`mesh_relay`, `mesh_introduce`, `mesh_peers/<id>`) are not shown to perform their own auth check — they delegate to `next.run(req).await` with zero verification. This was confirmed by inspecting `hook_routes.rs` for the pattern: hook endpoints at least have no auth by design (PTY context). For mesh relay/introduce there is no such justification in the code.

#### MEDIUM

- `auth.rs:47` — `identify_caller` calls `auth_header.strip_prefix("Bearer ").unwrap_or("")`. If the Authorization header value is malformed (e.g. just `"Bearer"` with a trailing space and nothing else) the function correctly returns `Err(())`. This is safe, but silently falling through to the peer-key check on an empty token is a subtle control flow that is worth documenting.

- `auth.rs:66-80` — The `mesh.lock()` is held while cloning `known_keys` and `paired_peers`, and then explicitly dropped before the call to `verify_signed_request`. This is correct. However, the intermediate variables require reading carefully: a future maintainer adding code between `known_keys.insert(...)` and `drop(mesh)` risks holding the lock across a potentially blocking operation. The pattern is correct today but fragile.

#### LOW

- `auth.rs:99-112` — `AgentIsolated` access level allows `POST /api/agents/spawn` and `POST /api/batch`. A rogue agent that has been given `AgentIsolated` access can spawn arbitrary child agents and issue arbitrary batched commands. This is intentional by design (agents need to coordinate), but there is no depth/spawn-count enforcement at the auth layer.

- `auth.rs:31-38` — `PUBLIC_PATHS` includes `/api/mesh/pair/request` through `/api/mesh/pair/reject`. These pairing handshake endpoints accept inbound connection attempts without any token. If the daemon is reachable from a network, any host can initiate a pairing request. This is expected for mDNS-based discovery but the auth layer has no rate-limiting guard.

---

### src-tauri/src/api/bridge_routes.rs

#### HIGH

- `bridge_routes.rs:149` — `bridge_start`: `state.svc.secrets.read().await` holds a `tokio::RwLock` read guard across an `await` point inside an `async` handler. This is correct for a `tokio::RwLock` (it is `await`-safe), but if `secrets` were ever switched to a `parking_lot::RwLock` (which the rest of the codebase uses) this would deadlock. The type mismatch is a latent hazard: all other locks in `AppState` use `parking_lot`; `secrets` is `tokio::RwLock`. The inconsistency should be documented.

#### MEDIUM

- `bridge_routes.rs:265` — `bridge_announce`: locks `state.svc.bridge_outbound.lock()` and holds the guard across a `tx.send(...)` call. The `send()` is non-blocking (it is a channel send, not a network call), so this is safe today. But if the outbound channel is ever replaced with one that blocks, the guard will extend across the block.

- `bridge_routes.rs:80-124` — `bridge_stop`: config is mutated (`cfg.integrations.telegram_enabled = false`) inside the lock while an `async` `.await` is pending immediately after (`bridge_supervisor.stop(...).await`). The `.await` is outside the lock scope (the lock is dropped correctly on line 93), but the early-return path at line 92 also correctly drops before returning. This is safe but the two separate `lock()` scopes for the same logical update is a DRY violation.

#### LOW

- `bridge_routes.rs:78` — `default_stop_timeout_ms` is 30 000 ms and `default_wait_for_lock_ms` is 10 000 ms. The timeout for `bridge_stop` (30s) is significantly longer than the HTTP client's typical default. Callers that do not set their own timeout will block for up to 30s with no intermediate progress feedback.

---

### src-tauri/src/api/config_routes.rs

#### HIGH

- `config_routes.rs:79` — `patch_config_api`: `*state.config.lock() = new_cfg.clone()` is called and then immediately `state.config.lock()` is called again for `prev_name`/`prev_mesh_enabled` on lines 72-73. The two separate `lock()` calls mean another thread could race and mutate config between the reads. The `prev_name` and `prev_mesh_enabled` values should be captured in a single lock scope before the write.

- `config_routes.rs:58-61` — `patch_config_api` partial update: the loop `for (k, v) in &body.partial` skips keys named `"key"` or `"value"` (to avoid merging the `PatchConfigBody` discriminator fields), but `body.partial` is the `#[serde(flatten)]` remainder which already excludes those fields by struct layout. The guard is redundant but harmless; more importantly, `partial` update merges any top-level JSON key into the config, including fields like `mesh.enabled` or `api.token`, without any field-level allowlist. A caller with a valid token can overwrite any config key by naming it in the partial map.

#### MEDIUM

- `config_routes.rs:125-133` — `dot_set` uses `parts.last().unwrap()` at line 132. The `unwrap` is safe because `parts.is_empty()` returns `Err(())` on line 127, so `parts` is guaranteed non-empty at line 132. But `.unwrap()` on an intermediate step in a `Result`-returning function is a code smell — `parts.last().ok_or(())` makes the invariant explicit.

#### LOW

- `config_routes.rs:136-159` — `bridge_send_api` lives in `config_routes.rs`. It is a bridge operation, not a config operation. It belongs in `bridge_routes.rs`.

---

### src-tauri/src/api/cron_routes.rs

#### HIGH

- `cron_routes.rs:17` — `cron_list`: uses `serde_json::to_value(jobs).unwrap()`. If the `CronJob` type ever adds a field that is not serializable (e.g. a raw function pointer), this panics in production. Use `.unwrap_or_default()` or map the error.

- `cron_routes.rs:27-28`, `cron_routes.rs:38-39`, `cron_routes.rs:50-51`, `cron_routes.rs:72-73`, `cron_routes.rs:82-83`, `cron_routes.rs:103`, `cron_routes.rs:128-129` — Multiple `.unwrap()` calls on `serde_json::to_value(...)` results scattered across nearly every handler. Same risk: any serialization failure panics the handler thread. The pattern is consistent but consistently wrong; a helper returning `Result` would fix all instances.

#### MEDIUM

- `cron_routes.rs:91-103` — `cron_trigger` error discrimination uses string matching (`e.contains("already running")`, `e.contains("not found")`). This couples the status code logic to error message text. If the service layer changes its wording, the status codes silently regress to 500.

#### LOW

- `cron_routes.rs:107-108` — The block comment for `cron_cancel` is a multi-line doc comment on a non-`pub` item. Consistent with the style rule violation pattern seen in `hook_routes.rs`.

---

### src-tauri/src/api/events.rs

#### MEDIUM

- `events.rs:91` — `serde_json::to_value(&event).unwrap_or(serde_json::Value::Null)` in `publish`: if the event fails to serialize, the webhook broadcast fires with a `null` payload. Consumers expecting a structured payload will silently receive `null`.

- `events.rs:104` — `std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default()`: `unwrap_or_default()` silently returns zero duration (epoch) if the system clock is before Unix epoch. Practically impossible but the `unwrap_or_default()` masks the anomaly rather than logging it.

- `events.rs:160-169` — Webhook dispatch spawns unbounded `tokio::spawn` tasks, one per matching hook per event. A burst of events against many registered webhooks creates an unbounded number of concurrent HTTP requests with no backpressure or circuit-breaker. If webhook targets are slow or unreachable, goroutines accumulate.

#### LOW

- `events.rs:115-137` — Two separate `self.webhooks.lock()` calls (one to collect matching hooks, one to remove one-shot hooks). Correct (lock is dropped between), but can race: another thread could re-add a one-shot hook between the two lock acquisitions. The race is benign (the newly added hook would not be fired for this event), but the two-pass pattern is fragile.

---

### src-tauri/src/api/health_routes.rs

#### OK

No issues found. Simple, clean. The `health` endpoint is correctly public (in `PUBLIC_PATHS`). `identify` correctly requires auth.

---

### src-tauri/src/api/hook_routes.rs

#### CRITICAL

- `hook_routes.rs:63-138` — `hook_permission`, `hook_stop` (lines 141-172), and `hook_session_start` (lines 178-207): **all three hook endpoints have no auth check**. The comment on line 57-58 says "No auth required — hooks run in the PTY process context." However, these endpoints are reachable from the network without any token. An attacker who can reach the API port can:
  - Inject arbitrary `session_id` values onto any pane via `hook_session_start`.
  - Clear agent state (`hook_stop`) on any pane, disrupting running agents.
  - Trigger the permission-granted notification flow (`hook_permission`) and potentially auto-approve a pending prompt by injecting an explicit `question`/`options` body.

  The "PTY process context" justification only holds if the API port is not network-accessible. In any mesh-connected or port-forwarded setup, the hooks are exposed. At minimum these should require the local bearer token, or be restricted to loopback-only via a separate listener.

#### HIGH

- `hook_routes.rs:88-91` — Two separate `state.svc.mux.lock()` calls in the same handler to get `pty_id` and `agent_type` on the same pane. Between the two locks, another thread could close the pane; the second lock then returns `None` for the pane. The values should be extracted in a single lock scope.

#### MEDIUM

- `hook_routes.rs:25` — `TODO(plugin-prep)` comment in production handler code. Per project rules, TODOs should not be left in production code.

---

### src-tauri/src/api/logs_routes.rs

#### OK

No issues. Both handlers authenticate, cap the limit, and handle the absent-store case explicitly. Error messages are consistent.

---

### src-tauri/src/api/mod.rs

#### OK

Exactly 25 lines — pure re-exports. Correct facade usage.

---

### src-tauri/src/api/network_routes.rs

#### MEDIUM

- `network_routes.rs:12-28` — `network_info`: no auth check. Any caller (local or network) can enumerate the machine's local IP, Tailscale IP, API port, and API URLs without a token. For a single-user local daemon this is low risk, but in a mesh scenario it leaks topology to unauthenticated peers who can reach the mesh-ping endpoint.

#### LOW

- `network_routes.rs:13` — `local_ip_address().unwrap_or_else(|| "127.0.0.1".to_string())` — silently falls back to loopback without logging, which makes the `api_url` field misleading (the actual local IP discovery may have failed).

---

### src-tauri/src/api/notification_routes.rs

#### CRITICAL

- `notification_routes.rs:62-92` — `serve_file`: **no auth check**. Any unauthenticated caller who can reach the API port can read any file on the filesystem by passing an arbitrary `path` query parameter. The handler does no path traversal validation, no allowlist check, and no restriction on file type. An attacker can read `/etc/passwd`, `~/.ssh/id_rsa`, the API token file, Telegram secrets, etc.

#### MEDIUM

- `notification_routes.rs:23` — `list_notifications` uses `serde_json::to_value(notifs).unwrap()`. Panics if `notifs` is not serializable.

#### LOW

- `notification_routes.rs:88-91` — `Response::builder().body(...).unwrap()`: the builder only fails if an invalid header value was already set, which cannot happen here since only `Content-Type` is set with a `'static` value. Safe in practice but the `unwrap` is avoidable.

---

### src-tauri/src/api/os_notify.rs

#### MEDIUM

- `os_notify.rs:19-103` — `_send_os_notification`: the entire fallback WebviewWindow path is inside an `#[allow(unreachable_code)]` block after a `return` that only exits if the notification plugin **succeeds**. This means in any realistic run where the plugin fails (e.g. sandboxed environment), the unreachable-code block actually executes. The `#[allow(unreachable_code)]` attribute is misleading — the code is very much reachable on the failure path.

- `os_notify.rs:44` — Two separate acquires of `LAST_MSG.lock()`: one to read (line 39) and one to write (line 44). Between these two locks another thread could change `LAST_MSG`, creating a TOCTOU race on the dedup check.

#### LOW

- `os_notify.rs` — All exported symbols have underscore prefixes (`_send_os_notification`, `_send_os_notification_if_unfocused`, `_build_notif_html`, `_get_workarea`, `_calc_notif_height`). This is unconventional in Rust; underscore prefixes mean "intentionally unused" by convention. Using `pub(crate)` and omitting underscores would be more idiomatic.

- `os_notify.rs:136-138` — `_get_workarea` is marked `#[allow(dead_code)]`. If it is always used (it is called from `_send_os_notification`), the allow is wrong. If it is sometimes unused, the function should be removed or gated.

---

### src-tauri/src/api/pane_routes.rs

#### HIGH

- `pane_routes.rs:272-313` — `exec_in_pane`: the inline `tokio::spawn` polling loop acquires `buf_clone.lock()` on lines 287, 290, 292 (three separate lock acquisitions across `await` points within the spawned future). Each acquisition is short (not held across await), which is correct. However, `buf_clone.lock()` is a `parking_lot::Mutex` — holding it at all on an async task that yields between iterations is safe only because each lock is immediately dropped. This is OK but the pattern is fragile and would deadlock if any lock acquisition were accidentally extended.

- `pane_routes.rs:279` — `timeout_ms` accepts any `u64` value from the caller with no upper bound. A caller can set `timeout_ms: 18446744073709551615` (u64::MAX), causing the `exec_in_pane` task to run for ~585 million years. Add a reasonable cap (e.g. 300 000 ms).

#### MEDIUM

- `pane_routes.rs:400-432` — `stream_pane_output`: the SSE stream loops forever (`loop { tokio::time::sleep(200ms).await; ... }`). There is no client-disconnect detection and no timeout. If the client disconnects without cleanly closing the SSE connection, the task runs forever and holds references to `buf` and `pid`.

- `pane_routes.rs:471-486` — `read_log_from_disk_by_pane`: the function holds `state.cron.lock()` (line 474) across a loop that calls `store.get_history()` and `store.read_log()`, which involve file I/O. Blocking file reads while holding a mutex is a latent deadlock risk if any other task tries to acquire `cron` while the file read is slow (e.g. NFS or spinning disk).

- `pane_routes.rs:52-71` — `SendTextBody` has seven fields with doc comments. This is an API boundary struct and comments are justified, but the comment on `source_pane` (lines 67-70) is four lines — violates the one-line comment rule.

#### LOW

- `pane_routes.rs:723` — File is 723 lines. Under the 1000-line hard limit but significantly large. The file mixes SSE streaming, exec polling, output reading, PTY management utilities, and text injection. These are separate concerns that would benefit from splitting.

- `pane_routes.rs:81-92` — `await_pty_link`: magic number `75` (iterations) × 200ms = 15s total wait. No constant or doc for this budget. Used in two handlers (`send_text`, `exec_in_pane`) but the 75-iteration limit is unclear without reading the arithmetic.

---

### src-tauri/src/api/routes.rs

#### HIGH

- `routes.rs:231-233` — `check_auth` is a no-op stub that always returns `Ok(())`. The comment says "Auth + access check handled by auth_middleware layer." This is architecturally correct for the middleware-based approach. However, it means that any handler that calls `check_auth` gets a false sense of security — the function does nothing. New routes that are added via a different router builder (e.g. a future plugin router that doesn't apply `auth_middleware`) would have `check_auth` calls that silently pass with zero enforcement. The no-op should at minimum be documented as a middleware delegation, not just a backwards-compat shim.

#### MEDIUM

- `routes.rs:58-59` — `keepawake_status_handler`, `keepawake_start_handler`, `keepawake_stop_handler` are registered under `/api/keepawake` but implemented in `network_routes.rs`. The naming mismatch (keepawake in network_routes) is a responsibility-placement inconsistency. Keep-awake is not a network concern.

- `routes.rs:109` — `bridge_send_api` is registered here as a config route (`config_routes::bridge_send_api`) but is a bridge operation. Confirmed as misplacement.

#### LOW

- `routes.rs:56-57` — Route `/api/webhooks/{id}` uses a fully-qualified `axum::routing::delete(...)` while most other routes use the `delete` import at the top. Minor inconsistency; similar for `axum::routing::delete` on lines 62, 132.

- `routes.rs:222-227` — `resolve_pane` and `check_auth` are `pub(crate)` helpers mixed into the router module. They belong in a separate `helpers.rs` or `middleware.rs` file. `routes.rs` is a router builder that should not own shared utility logic.

---

### src-tauri/src/api/services_routes.rs

#### MEDIUM

- `services_routes.rs:178-181` — `regenerate_token`: calls `state.svc.tokens.generate(&id)` and then `state.svc.services_store.update(...)` in two separate operations with no transaction. If the update fails after the token is generated, the new token is live but `push_enabled` is not set — the service is in an inconsistent state. The error from `update` is ignored with `let _`.

#### LOW

- `services_routes.rs:33-41` — `parse_activity_source` returns `None` for unknown source strings silently, causing unknown filter values to be treated as "no filter". A caller sending `source=typo` gets all activity instead of an error.

---

### src-tauri/src/api/session_routes.rs

#### MEDIUM

- `session_routes.rs:118-135` — Idempotency check for resume: acquires `state.svc.mux.lock()`, searches for existing pane, then immediately calls `mux.snapshot()` — all within the same lock scope. This is correct, but `snapshot()` likely clones significant state. Holding the lock while doing a full snapshot is a potential contention point if the mux is large.

- `session_routes.rs:153` — `state.svc.mux.lock().set_active_workspace(&id)` acquires and immediately drops `mux` for a single mutation. Two lines later, `state.svc.emit_state_changed()` is called. Another thread could observe the state change event before `set_active_workspace` is visible, but this is a general TOCTOU pattern across the codebase and not specific to this file.

#### LOW

- `session_routes.rs:189` — `meta.session_id[..8]` panics if `session_id` is fewer than 8 characters. Should use `.chars().take(8).collect::<String>()` or check the length.

---

### src-tauri/src/api/theme_routes.rs

#### MEDIUM

- `theme_routes.rs:36-38` — `save_theme_api`: the `body.id` field is used directly to construct a filename (`format!("{}.toml", body.id)`). No sanitization of the `id` value is performed. A caller can pass `id: "../../../etc/cron.d/evil"` to write a TOML file to an arbitrary path via directory traversal. The `body.id` must be validated (e.g. alphanumeric + dash only, no path separators).

#### LOW

- `theme_routes.rs:31-39` — `save_theme_api` parses the theme TOML to validate it, then writes the raw `body.toml` string to disk. The validated representation is discarded and the raw (potentially reformatted) input is stored. Consistent behavior, but if the TOML parser normalizes the content, the on-disk file and the in-memory parse may diverge.

---

### src-tauri/src/api/updater_routes.rs

#### MEDIUM

- `updater_routes.rs:35-37` — `check_update`: calls `state.update_state.lock()` twice in quick succession (lines 27 and 35) with different scopes. Between the two acquires, another task could mutate `update_state`. This is a benign race here (worst case: re-checks when cache says it's fresh), but the pattern should be consistent.

- `updater_routes.rs:124-152` — `install_update` with `method=auto`: spawns a `std::thread` that builds a new `tokio::runtime::Builder::new_current_thread()` to run `check_and_apply`. This creates a nested Tokio runtime inside a thread spawned from an async handler. While technically valid (the thread is outside the async context), it is an unusual pattern that bypasses the main executor's scheduler, backpressure, and cancellation mechanisms.

#### LOW

- `updater_routes.rs:93` — `install_update` accepts a `manifest_url` query parameter that can override the manifest URL. The comment says it's "gated behind `dev-updater-allowlocal` feature at the manifest fetch layer." This gating is not visible at the API boundary — the parameter is accepted unconditionally and passed through to `check_and_apply`. An audit of `check_and_apply` would be needed to confirm the gating is effective.

---

### src-tauri/src/api/usage_routes.rs

#### OK

Clean, consistent handlers. All authenticate, errors are propagated properly. `limit.clamp(1, 5000)` is a good practice seen in `usage_history`.

---

### src-tauri/src/api/webhook_routes.rs

#### MEDIUM

- `webhook_routes.rs:33-50` — `register_webhook`: the `url` field is accepted without validation. A caller can register a webhook pointing to an internal network address (`http://localhost/...`, `http://169.254.169.254/...`, etc.), turning the server into an SSRF proxy. Any event dispatch will make HTTP POST requests to attacker-controlled internal endpoints.

- `webhook_routes.rs:53-61` — `unregister_webhook`: deletes by `id` without verifying the caller owns the webhook. In a multi-caller scenario (mesh peers with different access levels), a `ReadOnly` peer who registered a webhook could have it deleted by another peer, and vice versa. No ownership tracking.

#### LOW

- `webhook_routes.rs:17-19` — `list_webhooks` uses `serde_json::to_value(hooks).unwrap()`. Same unwrap-on-serialize pattern found throughout the codebase.

---

### src-tauri/src/api/workspace_routes.rs

#### LOW

- `workspace_routes.rs:128` — `let _ = state.svc.set_pane_role(&pane_id, "Orchestrator", None, Some(true))` silently ignores the error from `set_pane_role`. If role assignment fails, the caller receives a success response but the pane is not properly configured. The error should at minimum be logged.

- `workspace_routes.rs:62-72` — `rename_workspace` maps both "not found" and any other dispatch error to `StatusCode::NOT_FOUND`. A serialization error or internal service error would also return 404, masking the real cause.

---

## Summary Table

| Severity | Count |
|----------|-------|
| CRITICAL | 2     |
| HIGH     | 14    |
| MEDIUM   | 20    |
| LOW      | 20    |
| **Total**| **56**|

### Critical Findings (summary)

1. **`notification_routes.rs` — `serve_file` has no auth**: unauthenticated arbitrary file read from the filesystem.
2. **`hook_routes.rs` — all three hook endpoints have no auth**: unauthenticated callers can inject session IDs, clear agent state, and trigger permission-approval flows on any pane.

### Highest-Priority Fixes

1. Add auth to `serve_file` or move it to a separate localhost-only listener.
2. Add local-token auth to `hook_permission`, `hook_stop`, `hook_session_start` — even if the hooks are called from a PTY subprocess, requiring the local token is trivial (the daemon injects `CODETERM_API_TOKEN` into every PTY environment).
3. Fix path traversal in `save_theme_api` — sanitize `body.id` before constructing the filename.
4. Add auth to `clear_prompt` and `set_agent_type` in `agent_routes.rs`.
5. Cap `timeout_ms` in `exec_in_pane`.
6. Replace all `serde_json::to_value(...).unwrap()` calls in production handlers with error-mapped variants.
