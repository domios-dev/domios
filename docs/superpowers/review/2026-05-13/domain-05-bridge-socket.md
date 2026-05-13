# Domain Review: Bridge & Socket

## Files Reviewed

- `src-tauri/src/bridge/mod.rs`
- `src-tauri/src/bridge/traits.rs`
- `src-tauri/src/bridge/sanitize.rs`
- `src-tauri/src/bridge/inbound_buffer.rs`
- `src-tauri/src/bridge/supervisor.rs`
- `src-tauri/src/bridge/manager/mod.rs`
- `src-tauri/src/bridge/manager/tests.rs`
- `src-tauri/src/bridge/response.rs`
- `src-tauri/src/bridge/telegram.rs`
- `src-tauri/src/bridge/voice_transcribe.rs`
- `src-tauri/src/bridge/text_hooks.rs`
- `src-tauri/src/bridge/migrate_test.rs`
- `src-tauri/src/socket/mod.rs`
- `src-tauri/src/socket/server/mod.rs`
- `src-tauri/src/socket/server/agent.rs`
- `src-tauri/src/socket/server/bridge.rs`
- `src-tauri/src/socket/server/cron.rs`
- `src-tauri/src/socket/server/notification.rs`
- `src-tauri/src/socket/server/pane.rs`
- `src-tauri/src/socket/server/services.rs`
- `src-tauri/src/socket/server/workspace.rs`

---

## Findings

### src-tauri/src/bridge/manager/mod.rs

#### HIGH

- **`manager/mod.rs:226-231` — infinite spin when both channels close**
  The `select! else` branch logs an error then `tokio::time::sleep(5s)` and loops. Once an `mpsc` channel is closed it stays closed; sleeping does not recover it. If both `rx` (inbound from Telegram service) and `outbound_rx` close while the manager task is still alive, this branch produces infinite log spam every 5 s until the cancel token fires. The comment "Channels may recover" is incorrect. Should `break` (or `return`) on `else`.

- **`manager/mod.rs:590-595` — `std::thread::spawn` with blocking sleep inside async task**
  A detached `std::thread` is spawned to send Enter keystrokes 500 ms and 2000 ms after paste. The thread holds a clone of `svc.pty` (a `parking_lot::Mutex`) and calls `.lock().write()` on it from a blocking thread while the async manager loop may also hold or contend the same lock. No cancel mechanism: if the bridge is restarted before 2 s, the orphaned thread still fires two Enter keypresses into the new PTY session, potentially corrupting the next agent's input.

- **`manager/mod.rs:79` — `unwrap_or_default()` produces empty token silently**
  `read_token().unwrap_or_default()` yields `""` if the token file is missing or unreadable. All subsequent Telegram API calls then use `bot/sendMessage` (no token), returning 401 errors that are silently retried. The caller (`start_telegram_bridge`) already guards against the missing-token case, but `manager/mod.rs` obtains the token again independently without any error path — if the token disappears after start, no log distinguishes "wrong URL" from "missing token".

#### MEDIUM

- **`manager/mod.rs:43-44` — misleading `_allowed_users` constructor parameter**
  `TelegramService::new(token, _allowed_users)` accepts a `Vec<String>` but immediately discards it (underscore-prefixed). `is_user_allowed()` reads from `load_config()` on every message instead. The constructor signature implies the caller controls the filter list; it does not. Remove the parameter or add an inline comment explaining the hot-reload design so the next caller does not assume it works.

- **`manager/mod.rs` — 610 lines; Telegram API call functions should be extracted**
  `send_message`, `send_message_with_buttons`, `edit_message`, `send_media`, `tg_url`, `send_typing` (lines ~237–419) are pure Telegram HTTP wrappers with no dependency on `BridgeManager` internals. They belong in a concrete `telegram_api.rs` file alongside the existing `telegram.rs`. Current `mod.rs` mixes orchestration (`start()`, `inject_message()`) with low-level API calls, violating the mod.rs-is-facade rule.

- **`manager/mod.rs:85` — `unwrap_or_else(|_| reqwest::Client::new())` in async hot path**
  Building an HTTP client inside the async `start()` method that silently falls back to a default client on builder failure. The builder failure would lose TLS/timeout/keepalive settings. Should propagate the error via `?` or pre-build the client before the loop.

#### LOW

- **`manager/mod.rs:131` — `user_name` logged before allowed-user check**
  Line 128-130 logs the inbound message (including `user_name` and first 100 chars of text) before verifying the user is allowed. The check happens in `inject_message` → `general_agent` lookup path, not here. For a rejected user (not in allow list), their name and partial message content reaches the log. Low risk (private bot, local log), but inconsistent with the allow-list pattern used in `telegram.rs:poll_updates`.

---

### src-tauri/src/bridge/sanitize.rs

#### LOW

- **`sanitize.rs:47` — `stack.pop().unwrap()` in production code**
  Inside the close-tag cleanup loop (`while stack.len() > pos + 1`), `stack.pop().unwrap()` is called. The `while` condition guarantees the stack is non-empty, so this cannot panic in practice, but it is a bare `unwrap()` in production code. Should be `expect("stack non-empty by loop invariant")` or restructured to use `if let Some`.

- **`sanitize.rs:445-449` — test name misleads: `span_other_class_stripped` does NOT strip the class**
  The test asserts `out.contains("class=")` — meaning an arbitrary class value like `"not-allowed"` is preserved verbatim in the output. `filter_attrs` allows `class` on `span` unconditionally. The test name says "stripped" but the assertion confirms it passes through. Either the name is wrong or the intent is wrong. Since Telegram ignores unknown class values harmlessly, this is cosmetic, but the test documents the wrong behavior.

---

### src-tauri/src/bridge/response.rs

#### LOW

- **`response.rs:146` — `stack.pop().unwrap()` in production code**
  Same pattern as sanitize.rs:47. Inside `while stack.len() > pos + 1`, the pop is safe by invariant, but is still a bare `unwrap()`.

- **`response.rs:10-46` — `capture_response` is a blocking sleep loop unsuited for the bridge**
  The function is synchronous, polls with `std::thread::sleep(500ms)` for up to 120 s, locking `output_buf` and `tracker` on every iteration. It is called from the socket server (on a `std::thread`), not from the async bridge path, so the blocking is acceptable there, but the function signature says nothing about this constraint. If ever called from an async context it would block the executor.

---

### src-tauri/src/bridge/supervisor.rs

#### MEDIUM

- **`supervisor.rs:55` — hardcoded `"telegram"` service name in `BridgeSupervisor::new()`**
  `BridgeRuntimeState::new("telegram")` is hardcoded. `BridgeSupervisor` is generic but cannot be reused for a second messaging service without forking or changing this constructor. The service name should be a constructor parameter so the type can be extended when a new `MessagingService` is added.

#### LOW

- **`supervisor.rs:83-91` — `set_task` resets all state on every re-use**
  `set_task` aborts any previous handle and resets `last_poll_ok`/`error_count`. This is correct behavior, but the abort happens with no drain/await on the previous task other than the `prev.abort()`. The caller in `startup/bridge.rs` does call `stop()` first (which awaits), so in practice the previous task is already done. However, calling `set_task` directly without stopping first (e.g. in future code) would silently leak the previous poller's state machine. A guard or assertion would prevent misuse.

---

### src-tauri/src/bridge/telegram.rs

#### HIGH

- **`telegram.rs:191` — `.parent().unwrap_or(Path::new("."))` as downloads dir fallback**
  If `secrets_dir()` returns a path with no parent (e.g. a root-level path), downloads fall back silently to `"."` (CWD), which may be the app bundle or an unexpected directory. This is low-probability but would produce confusing file paths. Should return an error or use a well-known fallback path instead of `"."`.

#### MEDIUM

- **`telegram.rs:43` — `_allowed_users` constructor parameter is dead**
  Same issue as `manager/mod.rs`: the constructor accepts and discards `_allowed_users`. See note under `manager/mod.rs`.

#### LOW

- **`telegram.rs:277-281` — silent defaults for missing callback_query fields**
  `cb_id`, `data`, `user_name` default to `""` / `"unknown"` / `0` when Telegram sends a malformed callback_query. A user_id of `0` passes `is_user_allowed` only if `0` is in the allow list; in that case a structurally malformed update would be forwarded to the agent. Very unlikely from Telegram's side, but warrants explicit logging at warn level rather than silent defaulting.

---

### src-tauri/src/bridge/voice_transcribe.rs

#### MEDIUM

- **`voice_transcribe.rs` — entire health-cache machinery duplicated from `text_hooks.rs`**
  `health_cache()`, `cached_health()`, `store_health()`, `build_client()`, `check_health()`, `is_valid_http_url()` are copy-pasted verbatim (minor type differences aside) in both files. A shared `bridge::daemon_health` module would eliminate ~80 lines of duplication and ensure both features get future improvements (e.g. negative-TTL decay on recovery) simultaneously.

#### LOW

- **`voice_transcribe.rs:122` — timeout clamped at `max(1, ...)` with no upper bound**
  `cfg.timeout_ms.max(1)` prevents zero but allows arbitrarily large values. A misconfigured daemon with `timeout_ms = 600_000` would block the async manager loop for 10 minutes (since `reqwest`'s timeout wraps around the `.send()` future). An upper bound (e.g. 60 s) would be safer.

---

### src-tauri/src/bridge/text_hooks.rs

#### MEDIUM

- **`text_hooks.rs` — health-cache machinery duplicated from `voice_transcribe.rs`**
  Same as voice_transcribe finding. Identical six helper functions; any fix must be applied to both files independently.

---

### src-tauri/src/bridge/inbound_buffer.rs

No findings. Buffer cap (100 messages per pane), FIFO drop-oldest policy, global `OnceLock<Mutex<BTreeMap>>` pattern are all correct. Tests are thorough and pass isolation between panes.

---

### src-tauri/src/bridge/traits.rs

No findings. `MessagingService` trait is a clean boundary; `InboundMessage`, `OutboundContent`, and `InlineButton` are well-typed. The `#[serde(alias = "data", rename = "callback_data")]` mapping is intentional and documented.

---

### src-tauri/src/socket/server/mod.rs

#### HIGH

- **`server/mod.rs:230-235` — no inbound line size limit on `BufReader::read_line`**
  `BufReader::read_line` reads until `\n` with no byte cap. A client (local process or script) that sends a single line of, say, 1 GiB of JSON will cause `line` to grow unboundedly, consuming heap until OOM. Since the socket is OS-protected (Unix domain), only processes running as the same user can connect, so exploitability is limited to a local misbehaving or compromised process. Still, a `take(MAX_LINE_BYTES)` guard is standard practice.

#### MEDIUM

- **`server/mod.rs:333-335` — `system.capabilities` advertises methods not implemented in socket**
  `agent.list`, `agent.hierarchy`, `agent.set_role`, `agent.clear_role`, `agent.status`, `agent.children`, `agent.link` are listed in the capabilities response but the `agent::dispatch()` only handles `agent.spawn`, `agent.report`, `agent.reports`. All others return `-32601 Method not found`. Capabilities should reflect what the socket actually implements, or the missing handlers need to be added.

- **`server/mod.rs` — 381 lines; `dispatch` and `handle_connection` could split**
  `socket_path()`, `socket_display_path()`, `SocketServer::cleanup_stale_sockets()`, `SocketServer::start()`, `handle_connection()`, `dispatch()`, and `get_string_param()` all live in `mod.rs`. `handle_connection` + `dispatch` together are the logical connection handler and belong in a `connection.rs` concrete file; socket lifecycle (`start`, `cleanup_stale_sockets`) could stay in `mod.rs` as facade.

#### LOW

- **`server/mod.rs:209-213` — accept loop errors are logged but do not break**
  `Err(e)` on `listener.incoming()` is logged and the loop continues. For a listener error (e.g. underlying socket descriptor closed), this produces an infinite log spam with no recovery. A consecutive-error count with a backoff or break threshold would be safer.

---

### src-tauri/src/socket/server/agent.rs

#### MEDIUM

- **`agent.rs:42-61` — 40 ms busy-poll loop blocks socket thread for up to 30 s**
  `agent.spawn` with `wait_ready_secs=30` (default) spins with `thread::sleep(40ms)` on the socket handler thread. The socket uses one-request-per-connection, so this does not block other clients. However, it holds a `std::thread` from the OS thread pool for up to 30 s under normal operation (agent readiness latency). A `tokio::sync::oneshot` receiver polled via a short-lived async block, or a separate wait thread, would be cleaner and would not tie up a pool thread.

---

### src-tauri/src/socket/server/bridge.rs

#### HIGH

- **`bridge.rs:101` — hardcoded `https://api.telegram.org` in `bridge.react` ignores `TELEGRAM_API_BASE`**
  `bridge.react` constructs the reaction URL as `format!("https://api.telegram.org/bot{}/setMessageReaction", token)` directly, bypassing `telegram_api_base()` which reads `TELEGRAM_API_BASE` from the environment. This means `bridge.react` always hits the real Telegram API even in test/staging environments where the env var overrides the base URL. Every other call site uses `telegram_api_base()` or `tg_url()`. Should use `crate::bridge::telegram::telegram_api_base()`.

#### LOW

- **`bridge.rs:64` — button parse failure logs the raw JSON value**
  `log::warn!("[bridge.send] failed to parse buttons: {} — raw: {}", e, v)` dumps the raw JSON value `v` to the log. If the caller passes sensitive data as a malformed button payload, it appears in plaintext in the log. Low risk in practice (the field is intended to be button data), but logging untrusted input verbatim is inconsistent with the care taken elsewhere.

---

### src-tauri/src/socket/server/cron.rs

No findings. Pattern is consistent: `try_state`, unwrap-to-error, delegate to `svc.cron_*`. Error handling is uniform across all 10 cron methods.

---

### src-tauri/src/socket/server/notification.rs

No findings. Consistent with the rest of the socket submodules.

---

### src-tauri/src/socket/server/pane.rs

No findings. Clean delegation to `dispatch::dispatch`. Consistent error code usage (602 for bad params, 603 for internal errors).

---

### src-tauri/src/socket/server/workspace.rs

No findings.

---

### src-tauri/src/socket/server/services.rs

No findings. Well-extracted `internal_err` / `invalid_params` helpers are the cleanest error-handling pattern in the socket domain.

---

## Summary Table

| Severity | Count |
|----------|-------|
| CRITICAL | 0 |
| HIGH | 5 |
| MEDIUM | 8 |
| LOW | 10 |
| **Total** | **23** |

### HIGH findings consolidated

1. `manager/mod.rs:226` — infinite spin when both mpsc channels close
2. `manager/mod.rs:590` — detached thread fires Enter into PTY 2 s after bridge restart, no cancel mechanism
3. `telegram.rs:191` — `.parent().unwrap_or(Path::new("."))` silently redirects downloads to CWD
4. `server/mod.rs:230` — no inbound line size limit; unbounded heap growth possible from local client
5. `bridge.rs:101` — `bridge.react` ignores `TELEGRAM_API_BASE`, always hits prod Telegram API
