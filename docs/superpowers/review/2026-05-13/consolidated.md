# CodeTerm Full Code Review — Consolidated Report
**Date:** 2026-05-13
**Scope:** All source files — src-tauri/src/, src/, cli/src/, crates/mem-store/src/
**Domains reviewed:** 12
**Total files reviewed:** ~425

---

## Executive Summary

| Severity | Count | Domains affected |
|----------|-------|-----------------|
| CRITICAL | 21 | API Core, Memory, Mesh, Orchestration, Supporting Services, Startup & Commands, CLI, mem-store crate, Frontend |
| HIGH | 73 | All except Agent & Plugin |
| MEDIUM | 146 | All 12 |
| LOW | 123 | All 12 |
| **Total** | **363** | |

The codebase is architecturally sound with clear module boundaries and a well-defined lock ordering regime. However, it carries two distinct layers of risk: **security holes in the API layer** (unauthenticated endpoints, path traversal, SSRF) and **safety violations in the Rust core** (lock ordering inversions, panics in production paths, atomic-transaction missing). The mesh subsystem has four CRITICAL security findings that are unmitigated. The CLI layer is the most finding-dense domain, driven by pervasive `unwrap_or_default()` silent failures and a hardcoded port that breaks dev workflows. The memory, orchestration, and mem-store crate have correctness bugs (silent data loss on alias rewire, lock-order deadlock risk, missing transactions) that require priority fixes before the graph/dreaming subsystem scales. Frontend is relatively clean but has two critical async state bugs in SecretsTab that are user-visible.

---

## Cross-Cutting Patterns

### Pattern 1: `serde_json::to_value(…).unwrap()` in production route handlers
- **Appears in:** Domain 2 (API Core), Domain 4 (Mesh), Domain 8 (Supporting Services), Domain 10 (CLI)
- **Description:** Bare `.unwrap()` on serialization — if any serializable type ever gains a non-serializable field (raw pointer, function pointer, custom `Serialize` impl that can fail), the tokio worker thread panics unrecoverably. Even where the types are trivially serializable today, the pattern violates the project convention and creates fragility on every future type evolution.
- **Examples:**
  - `api/cron_routes.rs:17,27-28,38-39,50-51,72-73,82-83,103,128-129` — every handler
  - `api/mesh_routes/mod.rs:106,115,125` — `mesh_status`, `mesh_peers`, `mesh_discover`
  - `api/notification_routes.rs:23` — `list_notifications`
  - `cli/src/cmd/print.rs:5` — `print_json` helper used by many commands

### Pattern 2: Unauthenticated or insufficiently-gated endpoints
- **Appears in:** Domain 2 (API Core), Domain 4 (Mesh), Domain 9 (Startup & Commands)
- **Description:** Multiple production API routes lack token authentication. The justifications vary (PTY context, mesh relay design, public pairing handshake) but collectively create an attack surface that is inconsistent with the threat model assumed by other routes.
- **Examples:**
  - `api/hook_routes.rs:63-207` — all three hook endpoints (permission, stop, session_start)
  - `api/notification_routes.rs:62` — `serve_file` (arbitrary filesystem read)
  - `api/agent_routes.rs:327-336` — `clear_prompt` and `set_agent_type`
  - `mesh_routes/pairing.rs:23-85` — `mesh_pair_accept` unauthenticated

### Pattern 3: Guard-then-unwrap fragility (`if is_some() { .unwrap() }`)
- **Appears in:** Domain 1 (Agent & Plugin), Domain 6 (Orchestration), Domain 9 (Startup & Commands)
- **Description:** Code checks `Option::is_some()` in one expression, then calls `.unwrap()` in a separate expression trusting the earlier check. Refactoring the surrounding logic can silently break the safety invariant, converting a logical guarantee into a runtime panic.
- **Examples:**
  - `plugin/agent_claude/provider/launch.rs:14`
  - `plugin/agent_codex/provider.rs:99`
  - `plugin/agent_opencode/provider.rs:198`
  - `multiplexer/state/role.rs:21-25,42,60`

### Pattern 4: Lock acquired multiple times for logically-atomic operations (TOCTOU)
- **Appears in:** Domain 2 (API Core), Domain 4 (Mesh), Domain 6 (Orchestration), Domain 8 (Supporting Services)
- **Description:** Read-then-write operations take multiple short lock acquisitions with gaps between them. Another thread can mutate state in the gap, making "pre vs. post" comparisons unreliable or creating race windows that silently break correctness.
- **Examples:**
  - `api/config_routes.rs:72-79` — `patch_config_api` reads `prev_name`/`prev_mesh_enabled` before holding the write lock
  - `mesh/discovery.rs:122-126` — `mdns_daemon` and `discovery_active` set in two separate locks
  - `service/pane.rs:14-16` — `close_pane` pre_focus/mutation/post_focus as three separate acquisitions
  - `api/hook_routes.rs:88-91` — pty_id and agent_type read in two separate mux locks

### Pattern 5: `mod.rs` used as logic file, not facade
- **Appears in:** Domain 1 (Agent & Plugin), Domain 3 (Memory), Domain 4 (Mesh), Domain 5 (Bridge & Socket), Domain 6 (Orchestration), Domain 7 (Config & Secrets), Domain 10 (CLI), Domain 11 (mem-store)
- **Description:** `mod.rs` files significantly exceed the 300-line advisory limit and contain business logic, not just re-exports. This is the most pervasive structural pattern in the codebase, affecting 8 of 12 domains.
- **Examples:**
  - `mesh/mod.rs` — 768 lines (2.5× over)
  - `plugin/mod.rs` — 639 lines (2.1× over)
  - `config/schema/mod.rs` — 751 lines (2.5× over)
  - `bridge/manager/mod.rs` — 610 lines (2× over)
  - `cli/src/cmd/mem/mod.rs` — 713 lines (2.4× over)
  - `mem/layers/mod.rs` — 368 lines; `mem/store/mod.rs` — 367 lines

### Pattern 6: Literal-string detection in violation of architecture rule
- **Appears in:** Domain 1 (Agent & Plugin), Domain 6 (Orchestration), Domain 8 (Supporting Services)
- **Description:** CLAUDE.md states: "Detection uses structural fingerprints, never literal strings — vendors change copy without notice." Multiple detection functions use hardcoded verbatim strings from Claude's output, which silently break when upstream copy changes.
- **Examples:**
  - `plugin/agent_claude/provider/detection.rs:18-28` — `"Do you want to proceed?"`, `"Claude Code"`, etc.
  - `service/general_agent/watchdog.rs:5-27` — `"Claude Code"`, `"? for shortcuts"`, `"Cogitated for"`, `"Brewed for"`
  - `usage/probe.rs:241-251` — `"│ >"`, `"❯"`, `"ctrl+l"` for TUI input detection

### Pattern 7: `reqwest::Client` built fresh per-call with `unwrap_or_default()` fallback
- **Appears in:** Domain 4 (Mesh), Domain 5 (Bridge & Socket), Domain 9 (Startup & Commands), Domain 10 (CLI)
- **Description:** `reqwest::Client::builder().build().unwrap_or_default()` (or `Client::new()` fallback) is called repeatedly per request rather than building once and reusing. The fallback silently drops all TLS/timeout/header configuration. Repeated client construction wastes memory (each client has its own connection pool).
- **Examples:**
  - `mesh/mod.rs:385` — in `broadcast_sync`, `broadcast_introductions`, `broadcast_announce`
  - `commands_mesh.rs:78,114,213,244`
  - `cli/src/main.rs:144` — drops auth token header on fallback

### Pattern 8: DRY violation — duplicated utility functions across modules
- **Appears in:** Domain 1 (Agent & Plugin), Domain 5 (Bridge & Socket), Domain 8 (Supporting Services), Domain 10 (CLI)
- **Description:** Small utility functions are copy-pasted across modules rather than extracted to a shared location.
- **Examples:**
  - `month_from_name` / `month_from_short` — `agent_claude/usage_tui/resets.rs` and `agent_codex/status_tui.rs`
  - `find_file_by_suffix` / `find_file_by_name` — `agent_codex/provider.rs` and `agent_opencode/provider.rs`
  - `health_cache` / `cached_health` / `store_health` / `build_client` / `check_health` / `is_valid_http_url` — `bridge/voice_transcribe.rs` and `bridge/text_hooks.rs` (~80 lines verbatim)
  - `now_ms()` — six copies across `activity.rs`, `health/http.rs`, `health/tcp.rs`, `health/process.rs`, `health/script.rs`, `services/store/crud.rs`
  - `fmt_ts` / `format_unix_ts` — `cli/src/cmd/usage.rs` and `cli/src/cmd/sessions.rs`

---

## All CRITICAL Findings

### Domain 1 — Agent & Plugin
(none)

### Domain 2 — API Core Routes
- `api/notification_routes.rs:62-92` — `serve_file`: no auth check; unauthenticated caller can read any file on the filesystem by passing an arbitrary `path` query parameter (no path traversal validation, no allowlist)
- `api/hook_routes.rs:63-207` — `hook_permission`, `hook_stop`, `hook_session_start`: all three have no auth check; unauthenticated callers can inject session IDs, clear agent state, and trigger permission-approval flows on any pane

### Domain 3 — Memory
- `mem/dreaming.rs` — 1096 lines (allowlist cap: 1095; must shrink not grow — any addition is a CI violation)

### Domain 4 — Mesh
- `mesh/mod.rs:191` — Nonce is signed and included in tokens but never recorded in a seen-nonce set; any intercepted token is replayable for up to 300 seconds on any authenticated mesh endpoint
- `api/mesh_routes/relay.rs:89-97` — `path_requires_full_access` allowlist covers only 3 paths; a paired Full-access peer can relay to `/api/mesh/pair/request`, `/api/mesh/introduce`, `/api/agents/spawn`, and every other local API endpoint
- `api/mesh_routes/relay.rs:131` — Access level defaults to `Full` when peer not found after auth (TOCTOU between auth check and access lookup); should default to `ReadOnly`
- `api/mesh_routes/pairing.rs:23-85` — `mesh_pair_accept` is unauthenticated; any host that knows or intercepts the `request_id` UUID can complete pairing with a forged public key (MITM in pairing)
- `api/mesh_routes/introduction.rs:10-95` — `mesh_introduce` auto-pairs the introduced peer at `AccessLevel::Full` with no user confirmation; any single trusted paired peer can force-pair arbitrary instances

### Domain 5 — Bridge & Socket
(none)

### Domain 6 — Orchestration Core
- `service/pty_lifecycle.rs:166-167` — `handle_pty_death` acquires `agent_tracker` (lock 4) before `mux` (lock 1); mandated order is mux(1) → agent_tracker(4); this is a lock ordering violation that can deadlock any code path holding mux while a backend reader exits
- `spawn.rs` — `spawn_hidden_agent_impl` nested thread reproduces the same `agent_tracker` → `mux` inversion via `start_backend_reader` → `handle_pty_death`
- `orchestrator.rs:356-380` — Orphaned pane on partial spawn failure: pane is created and partially mutated before role assignment; if `set_pane_role` fails, the pane leaks in the mux indefinitely with no cleanup path (same pattern in `spawn.rs::spawn_visible_agent_impl`)

### Domain 7 — Config & Secrets
(none)

### Domain 8 — Supporting Services
- `usage/projection.rs:231` — `remaining_ms as f64` cast after `.max(0)` is safe, but `dt_ms` at line 222 has no guard before being used as a divisor; if rows are out of order in the DB (anomalous but possible), `dt_ms` is negative and the rate is negative — the `rate_per_ms <= 0.0` guard catches the result but not the root cause; additionally, `local_start_of_day_ms` returns `now_ms` itself during DST gaps, silently producing zero-delta usage queries for up to 6 hours

### Domain 9 — Startup & Commands
- `commands/completions.rs:13` — explicit `panic!()` in a `#[tauri::command]`-annotated function; crashes the Tauri process entirely on broken symlinks or permission denied
- `startup/window.rs:21` — `ns_window().unwrap()` on macOS; panics if Tauri returns `None` for the native window handle before window is fully created

### Domain 10 — CLI
- `cli/src/cmd/worker.rs:96-101` — `unsafe transmute` of SQLite extension function pointer; UB risk if C ABI doesn't match (documented workaround but no safety proof)
- `cli/src/cmd/worker.rs:452-458` / `cli/src/cmd/graph_mutate.rs` — `build_skill_id` slices UTF-8 by byte index at offset 40; panics on multi-byte characters in user-controlled skill names
- `cli/src/cmd/bridge/migrate.rs:121-131` — Migration sequence disables source before confirming target can claim; intermediate kill leaves source disabled with no automatic recovery; `--no-rollback` makes this permanent
- `cli/src/main.rs:144` — `build_http` failure silently falls back to unauthenticated client, dropping auth token and all custom headers; subsequent API calls succeed (no error) but are unauthenticated

### Domain 11 — mem-store Crate
- `crates/mem-store/src/graph.rs:384-416` — `add_alias_with_rewire` has no transaction; five separate `conn.execute()` calls run bare; failure mid-sequence leaves graph permanently inconsistent (alias exists, relations partially rewired, delete cannot roll back)
- `crates/mem-store/src/graph.rs:280-298` — `canonicalize_relation_types` has no transaction; a failure mid-loop leaves previous synonym pairs committed and later ones un-applied

### Domain 12 — Frontend
- `SecretsTab.tsx:363-375` — `lock()` and `logout()` have no try/catch around `await fetch()`; unhandled rejection silently leaves UI in inconsistent state (credentials appear cleared, may not be)
- `SecretsTab.tsx:662-669` — React async state race: `setMasterPassword("dev-only")` queues a state update; `submitPasswordLogin` runs synchronously in the same tick and captures the old `masterPassword` value (`""`), submitting an empty password

---

## All HIGH Findings

### Domain 1 — Agent & Plugin
(none)

### Domain 2 — API Core Routes
- `api/agent_routes.rs:159` — `serde_json::to_value(info).unwrap()` in `get_pane_status`; panics on serialization failure
- `api/agent_routes.rs:214` — `serde_json::to_value(&reports).unwrap_or_default()` in `get_reports`; returns 200 with null body on failure
- `api/agent_routes.rs:288-290` — `retry_after_secs.to_string().parse().unwrap()` in `agent_restart_handler`; unnecessary unwrap in hot path
- `api/agent_routes.rs:327-336` — `clear_prompt` and `set_agent_type` have no `check_auth` call; any unauthenticated caller can clear prompts or change agent type on any pane
- `api/auth.rs:43` — `is_custom_auth` skips auth for ALL `/api/mesh/peers/<anything>` paths except the literal `/api/mesh/peers`; state-mutating POSTs to peer addresses bypass token check
- `api/bridge_routes.rs:149` — `secrets` uses `tokio::RwLock` while all other `AppState` locks use `parking_lot`; inconsistency is a latent hazard if ever changed
- `api/config_routes.rs:58-61` — `partial` config update allows overwriting any top-level JSON key (including `mesh.enabled`, `api.token`) without field-level allowlist
- `api/config_routes.rs:79` — `patch_config_api` reads `prev_name`/`prev_mesh_enabled` before acquiring write lock; race between reads and write
- `api/cron_routes.rs:17,27-28,38-39,50-51,72-73,82-83,103,128-129` — multiple `.unwrap()` calls on `serde_json::to_value(...)` scattered across every handler
- `api/hook_routes.rs:88-91` — two separate `mux.lock()` calls to get `pty_id` and `agent_type`; pane could close between acquisitions
- `api/pane_routes.rs:272-313` — `exec_in_pane` inline polling loop with three separate parking_lot lock acquisitions across await points (fragile pattern)
- `api/pane_routes.rs:279` — `timeout_ms` accepts `u64::MAX` (585 million years); no upper bound enforced
- `api/theme_routes.rs:36-38` — `body.id` used unsanitized to construct a filename; directory traversal allows writing TOML to arbitrary paths
- `api/webhook_routes.rs:33-50` — webhook `url` field accepted without validation; SSRF vector (attacker-controlled internal endpoints receive event POST requests)

### Domain 3 — Memory
- `mem/rerank.rs` — `call_anthropic`/`call_openai` use `reqwest::blocking::Client`; blocking HTTP on tokio worker thread (200ms–5s latency) starves async runtime; not wrapped in `spawn_blocking`
- `mem/graph.rs` (add_alias) — `UPDATE OR IGNORE` on relation rewire silently skips conflicting rows; subsequent unconditional `DELETE` removes relations that were never moved to canonical entity (permanent data loss)

### Domain 4 — Mesh
- `mesh/mod.rs:593-600` — `refresh_local_addresses` takes 4 separate lock acquisitions; "prev" and "new" snapshots are not atomic; change detection can signal wrong
- `mesh/mod.rs:252-258` — `prepare_introductions` signs one token for all outbound introduction calls; if broadcast takes >300s the later introductions present expired tokens and are silently rejected
- `mesh/discovery.rs` (699 lines) — approaching 1000-line limit with four distinct responsibilities not separated; UDP listener has no per-source rate limit on beacon spam (DoS vector)
- `mesh/discovery.rs:534` — UDP beacon listener reads into 1024-byte fixed buffer; no per-source rate limit; spam can grow `discovered_peers` without bound
- `api/mesh_routes/relay.rs:162` — `cmd.path` passed directly into `local_url` with no validation that it starts with `/api/`; path traversal against local HTTP is possible
- `api/mesh_routes/relay.rs:67` — `.unwrap()` on `serde_json::to_value(our_payload)` in `mesh_sync`
- `api/mesh_routes/relay.rs:166-180` — relay re-signs with local identity and forwards to itself; access level enforcement on relay is the only guard; any gap in `path_requires_full_access` grants full local authority
- `api/mesh_routes/pairing.rs` (rate limiting) — `mesh_pair_request` has no rate limit; flooding peer can keep pending queue saturated (5 entries), preventing legitimate pairing
- `api/mesh_routes/connection.rs:78-152` — `mesh_approve_peer` (REST) does not call `broadcast_introductions`; REST and Tauri approval paths produce different mesh topology outcomes
- `api/mesh_routes/connection.rs:247` — `mesh_exit` uses `.addresses.first()` instead of address-tier selection; silent failure if first address is unreachable

### Domain 5 — Bridge & Socket
- `bridge/manager/mod.rs:226-231` — infinite spin when both mpsc channels close; `else` branch logs error and sleeps 5s forever; should `break` on closed channel
- `bridge/manager/mod.rs:590-595` — detached `std::thread` fires Enter keystrokes into PTY 2s after paste with no cancel mechanism; orphaned thread fires into new PTY session after bridge restart
- `bridge/telegram.rs:191` — `.parent().unwrap_or(Path::new("."))` silently redirects downloads to CWD on root-level path
- `socket/server/mod.rs:230-235` — no inbound line size limit on `BufReader::read_line`; 1 GiB line causes unbounded heap growth from local client
- `socket/server/bridge.rs:101` — `bridge.react` hardcodes `https://api.telegram.org` ignoring `TELEGRAM_API_BASE` env var; always hits prod even in test environments

### Domain 6 — Orchestration Core
- `service/pane.rs:14-16` — `close_pane` acquires `mux` three separate times; TOCTOU on pre/post focus comparison (same pattern in `close_workspace`, `close_tab`)
- `service/workspace.rs:12-35` — `create_workspace` existence check and creation are in separate lock acquisitions; concurrent creation of same cwd-named workspace can produce duplicates
- `orchestrator.rs` — orphaned pane on partial spawn failure (see CRITICAL)
- `orchestrator.rs:438-454` — `launch_agent_in_background`: nested `std::thread::spawn` → tokio runtime → `handle_pty_death` reproduces `agent_tracker` → `mux` ordering violation
- `service/general_agent/lifecycle.rs:75-85` — `start_general_agent` acquires `mux` multiple times sequentially; not a violation but fragile
- `service/pty_lifecycle.rs:60-162` — `start_backend_reader` holds `svc.pty.lock()` while calling `pair.lock().unwrap()` (std Mutex); panics on mutex poison
- `spawn.rs:381-470` — PTY spawned and linked before backend reader starts; if reader fails, PTY runs silently with no reader and no cleanup (pane appears live but is deaf)
- `spawn.rs:472-702` — same issue for `spawn_hidden_agent_impl`; run_watcher eventually timeouts but pane is never cleaned up on early reader failure

### Domain 7 — Config & Secrets
- `secrets/bitwarden/subprocess.rs:166-172` — `bw` session token logged in plaintext on JSON parse failure (fallback logs `stdout.trim()` which may contain raw session token)
- `secrets/bitwarden/secret_store.rs:159-163` — 2FA/TOTP code passed in `argv`; visible via `ps aux` / `/proc/<pid>/cmdline` for subprocess duration
- `secrets/bitwarden/secret_store.rs:325` — name validation silently discarded in `get()` (`validate_name(name_or_id).ok()`); NUL injection into subprocess argument not blocked

### Domain 8 — Supporting Services
- `services/store/crud.rs:166,169` — SQL filters for `kind` and `enabled` are interpolated directly into SQL string using `format!()`; structurally SQL injection even if safe today with closed enum
- `usage/projection.rs:513-514` — `BASELINE_CACHE` is process-global `OnceLock<Mutex<…>>` with no size bound; grows without limit on multi-account key space
- `usage/projection.rs:513-515` — projection cache uses `std::sync::Mutex` (poisoning) while rest of codebase uses `parking_lot`; inconsistency hazard
- `usage/projection.rs:758-765` — `local_start_of_day_ms` returns `now_ms` during DST spring-forward gap; usage queries return zero rows for up to 6 hours
- `update/apply.rs:83-96` — update phase set to `Applying` after binary is already swapped; crash between swap and phase write leaves no persistent record of successful apply (no boot-loop detection)

### Domain 9 — Startup & Commands
- `startup/cli_install.rs:73-88` — non-atomic binary install: crash between `copy` and `set_permissions` leaves non-executable binary; hash check permanently skips re-install
- `startup/servers.rs:95` — `std::env::set_var("CODETERM_API_PORT", ...)` called inside spawned thread while other threads read environment; data race (UB in POSIX, `unsafe` in Rust 2024 edition)

### Domain 10 — CLI
- `cli/src/main.rs:505-575` — Theme commands hardcode port `7685`, use unauthenticated `Client::new()` (no token, no timeout); always target prod regardless of `CODETERM_API_PORT`
- `cli/src/main.rs:512,542` — `resp.json().unwrap_or_default()` on theme responses; non-JSON returns null, prints nothing with exit code 0
- `cli/src/client.rs:107-119` — `SocketClient::call()` uses `read_line()` with no timeout; hung daemon makes every socket-based CLI command hang indefinitely
- `cli/src/cmd/mem/mod.rs:49` — `resp.json().unwrap_or_default()` in `mem_post_request`; non-JSON (gateway error page) silently becomes null
- `cli/src/cmd/mem/mod.rs:666-691` — `run_mem_tail` follow mode has no timeout; continues polling forever when daemon has gone away
- `cli/src/cmd/agent.rs:12,19,26` — `resp.json().unwrap_or_default()` on every agent list/hierarchy/providers response; non-JSON → null → exit 0 with no output
- `cli/src/cmd/agent.rs:365-390` — pane lookup errors silently swallowed via `.ok()`; failed agent-type lookup sends `/model <name>` anyway
- `cli/src/cmd/mesh.rs:25` — `resp.json().unwrap_or_default()` on status response; non-JSON → user sees `? (?)` with exit code 0
- `cli/src/cmd/mesh.rs:33` — `id[..id.len().min(8)]` slices by byte offset; panics on non-ASCII IDs
- `cli/src/cmd/bridge/migrate.rs:208-229` — post-migration verification triggers spurious rollback on a single transient network error (should retry before rollback)
- `cli/src/mesh_cli/relay.rs:115-116` — `resp.bytes().unwrap_or_default()` silently returns empty on network failure; actual error never shown
- `cli/src/mesh_cli/api_map.rs:63` — `workspace move-tab` not matched; silently sends GET to `/api/workspaces` instead of correct endpoint
- `cli/src/mesh_cli/api_map.rs:120` — most `AgentAction` variants fall to `_ => GET /api/agents`; `agent report --mesh` silently fetches agent list instead of reporting

### Domain 11 — mem-store Crate
- `crates/mem-store/src/graph.rs:80-104` — `purge_invalidated`: count taken outside transaction, delete inside; audit log records stale count (overcount/undercount possible)
- `crates/mem-store/src/graph.rs:151-184` — `revive_broken`: same pre-tx count pattern; returned `ReviveBrokenReport::revived` is unreliable
- `crates/mem-store/src/graph.rs:224-231` — `introduce_topic` silently accepts nonexistent anchor; creates graph edges to a ghost anchor (no FK enforcement)
- `crates/mem-store/src/graph.rs:148` — `route()` always reports `edges_created: 2` regardless of `INSERT OR IGNORE` outcome; false positive on idempotency check
- `crates/mem-store/src/lesson.rs:59` — `serde_json::to_string` failure fallback drops all pre-existing tags (data-destructive silent default)
- `crates/mem-store/src/error.rs` + `lib.rs` — entire public API bound to `rusqlite::Connection`; no abstraction layer; swapping storage backend requires touching every callsite

### Domain 12 — Frontend
- `ChatView.tsx` — stale closure: `handleKeyDown` dep array lists `[sendMessage]` but calls `toggleExpand` (not included); Escape key operates on stale state after collapse/expand
- `SchemaRenderer.tsx:44,83,215` — `key={i}` index for dynamically-ordered plugin schema fields; reorder causes identity loss and lost input values
- `MemoryPanel.tsx` — dynamic `import('@tauri-apps/api/event')` inside `useEffect` with no `.catch()`; silent failure, activity feed never populates
- `MeshPanel.tsx` — declares `api`, `config`, `onConfigUpdate` in Props interface but component only destructures `onClose`; all other props silently unused
- `BrowserPane.tsx` — hardcodes prod port `7685` in `resolveUrl`; breaks in dev environment and on non-default port config

---

## Per-Domain Summary

### Domain 1 — Agent & Plugin
The agent and plugin subsystem is the healthiest domain in the review. Lock discipline is clean (no lock held across await, no ordering violations). The structural fingerprinting architecture is implemented consistently in `detector.rs`. The main issues are: (1) `provider.rs` at 903 lines is approaching the 1000-line hard limit and needs a schema types extraction, (2) `plugin/mod.rs` at 639 lines violates the mod.rs-as-facade rule, (3) three copies of the guard-then-unwrap pattern in provider launch files, and (4) two DRY violations (month-name parsers, recursive file walk helpers). No CRITICAL or HIGH findings.

### Domain 2 — API Core Routes
The highest-severity domain outside mesh. Two CRITICAL findings: the unauthenticated `serve_file` endpoint (arbitrary filesystem read) and three hook endpoints with no auth. Fourteen HIGH findings covering serde panics, auth gaps on write endpoints, a path traversal in theme route, and an SSRF vector in webhook registration. The `check_auth` stub pattern (always returns `Ok(())`), while correctly delegating to middleware, creates false confidence and is a latent hazard for routes added via future plugin routers. Partial-update config (no field allowlist) allows any caller with a valid token to overwrite any config key including `api.token`.

### Domain 3 — Memory
Functionally solid but carrying two HIGH correctness issues: blocking HTTP on tokio thread in `rerank.rs` (async starvation), and silent data loss in `graph::add_alias` when `UPDATE OR IGNORE` skips rows that are then unconditionally deleted. The `dreaming.rs` file is 1 line over its allowlist cap (CRITICAL by CI rules). Eight MEDIUM findings including: write-on-every-search inflating worker access counts, hardcoded 30-minute watchdog ignoring config, `conn.prepare()` inside BFS loops, and two mod.rs files over the advisory limit.

### Domain 4 — Mesh
The highest security risk domain. Four CRITICAL findings: replay attack (nonces never stored), relay path allowlist incomplete (full local API reachable via relay), `mesh_pair_accept` unauthenticated (MITM in pairing), and `mesh_introduce` auto-pairs without user consent. Ten HIGH findings including non-atomic address refresh, shared token for multi-peer broadcasts, UDP beacon DoS, and divergent REST/Tauri approval code paths. The security issues compound each other: a single compromised Full-access peer can replay tokens, relay arbitrary commands, and introduce new peers without user awareness.

### Domain 5 — Bridge & Socket
No CRITICAL findings. Five HIGH findings: infinite spin on closed channels, orphaned thread firing Enter into new PTY sessions after bridge restart, download path fallback to CWD, unbounded socket line reads, and hardcoded Telegram API URL ignoring env override. Health-cache machinery is copy-pasted verbatim between `voice_transcribe.rs` and `text_hooks.rs` (~80 lines). The socket capabilities list advertises 7 methods that return `Method not found`. Overall architecture (traits, inbound buffer, supervisor) is clean.

### Domain 6 — Orchestration Core
Three CRITICAL findings: the `agent_tracker` before `mux` lock ordering violation in `handle_pty_death` (deadlock risk), the same inversion via nested threads in `spawn_hidden_agent_impl`, and orphaned panes on partial spawn failure. Eight HIGH findings including TOCTOU on close operations, duplicate workspace creation race, `std::sync::Mutex` poison panic in PTY backend reader, and PTY-without-reader state on failed reader start. The orchestration domain also has the most MEDIUM findings (21) driven by pervasive DRY violations (workspace resolution duplicated 4 times, env-var block duplicated 3 times, role-suffix block duplicated 2 times), blocking `parking_lot::Mutex::lock()` inside async tasks, and the 835-line `orchestrator.rs` that needs splitting.

### Domain 7 — Config & Secrets
No CRITICAL findings. Three HIGH findings: Bitwarden session token logged in plaintext on fallback path, 2FA TOTP code in process argv (visible via `ps`), and NUL injection not blocked in Bitwarden `get()`. `SecretString` correctly implements `Zeroize`; secrets are never serializable; the index never stores values. Main structural issue is `config/schema/mod.rs` at 751 lines with 15+ struct definitions, 10+ serde helpers, and a full theme catalog that should each be separate files. Default `ApiConfig.host = "0.0.0.0"` exposes the REST API on all network interfaces on fresh install.

### Domain 8 — Supporting Services
One CRITICAL finding in `projection.rs`: DST spring-forward causes `local_start_of_day_ms` to return `now_ms`, silently producing zero-delta usage queries for up to 6 hours. Five HIGH findings: SQL injection pattern in services CRUD `list()`, unbounded projection cache, `std::sync::Mutex` inconsistency in projection, DST cycle-day bug, and non-atomic session file save. Health scheduler probes all services sequentially with no concurrency or timeout budget. Six copies of `now_ms()` across the supporting services modules.

### Domain 9 — Startup & Commands
Two CRITICAL findings: explicit `panic!()` in `completions.rs` Tauri command handler (process crash on broken symlink), and `ns_window().unwrap()` in macOS window setup. Two HIGH findings: non-atomic CLI binary install and `std::env::set_var` in a spawned thread (data race / UB). The startup split into `states.rs`/`servers.rs`/`background.rs`/`window.rs` is architecturally appropriate. Main weakness is the 2-second hardcoded sleep before GA start (no handshake). `commands_mesh.rs` at the top level rather than inside `commands/` is a structural inconsistency.

### Domain 10 — CLI
The most finding-dense domain (58 total). Four CRITICAL findings: `unsafe transmute` for SQLite extension, byte-index UTF-8 slice panic in `build_skill_id`, bridge migration split-brain with no atomic recovery, and silent auth token drop on `build_http` fallback. Thirteen HIGH findings dominated by `resp.json().unwrap_or_default()` silent failures (exit 0 with no output on API errors), hardcoded port 7685 in Theme commands, no socket timeout, and broken mesh `api_map.rs` command routing. The CLI would benefit significantly from a central error-handling discipline: every API response should go through a helper that checks HTTP status before deserializing, surfaces errors to the user, and exits with a non-zero code.

### Domain 11 — mem-store Crate
Two CRITICAL findings: `add_alias_with_rewire` and `canonicalize_relation_types` have no wrapping transactions; multi-step mutations are non-atomic and can leave the graph permanently inconsistent. Six HIGH findings including stale audit counts (count taken outside transaction), ghost-anchor edges on nonexistent anchor, false edge-creation counts, and the public API being entirely bound to `rusqlite::Connection`. The crate is small (12 files) but is the foundation of the graph layer; correctness here is essential. Tests are comprehensive but miss the three untested edge cases identified.

### Domain 12 — Frontend
Two CRITICAL React bugs in `SecretsTab.tsx` (unhandled promise rejections and async state race). Five HIGH findings: stale closure in `ChatView`, index keys for dynamic plugin fields, silent `listen()` setup failure in `MemoryPanel`, dead props in `MeshPanel`, and hardcoded prod port in `BrowserPane`. The frontend is otherwise well-structured with a clear component hierarchy. The main systemic issues are: inconsistent error-handling patterns (three coexisting approaches: `setError`, `console.error`, `alert()`), 10 uses of blocking `window.alert()` for errors, and pervasive `useCallback` dep arrays that include broad state objects causing unnecessary re-renders.

---

## Systemic Recommendations

### 1. Security sprint: close the unauthenticated endpoint gaps
**What:** Add token auth to: `serve_file`, all three hook endpoints (`hook_permission`, `hook_stop`, `hook_session_start`), `clear_prompt`, `set_agent_type`, and `network_info`. Sanitize `body.id` in `save_theme_api` (path traversal). Add URL validation in `register_webhook` (SSRF). Fix relay path allowlist to cover all state-mutating mesh endpoints.
**Why:** These are the highest-impact exploitable findings; the hooks CRITICAL and file-serve CRITICAL are reachable from any process that can reach the API port, including mesh-connected peers.
**Estimated impact:** 2 CRITICAL, 4 HIGH eliminated.

### 2. Fix mesh security model: nonce recording, pairing auth, introduction consent
**What:** Record seen nonces in a bounded TTL set (e.g., `DashMap<String, Instant>`, prune entries older than 300s). Add auth to `mesh_pair_accept` (at minimum require the original request's nonce and peer signature). Add user-visible confirmation to `mesh_introduce` before auto-pairing.
**Why:** The four mesh CRITICAL findings compound each other; a single compromised peer can replay tokens AND introduce arbitrary peers without the user's knowledge. These three fixes together close all four CRITICAL items.
**Estimated impact:** 4 CRITICAL, 3 HIGH eliminated.

### 3. Fix lock ordering violation in `handle_pty_death`
**What:** In `pty_lifecycle.rs::handle_pty_death`, move `agent_tracker.set_dead(pty_id)` to after `mux.lock().pane_id_for_pty(pty_id)`. The `set_dead` call only needs the `pty_id`; it does not need to happen before the pane lookup. Update `spawn.rs` nested thread and `orchestrator.rs::launch_agent_in_background` to verify they no longer reproduce the inversion.
**Why:** This is the one true lock ordering violation in the codebase. It can cause deadlock on any agent exit under concurrent load — the mandated order exists precisely to prevent this class of bug.
**Estimated impact:** 2 CRITICAL, 2 HIGH eliminated.

### 4. Add missing transactions to `mem-store` graph mutations
**What:** Wrap `add_alias_with_rewire` and `canonicalize_relation_types` in `conn.unchecked_transaction()` like every other multi-step write in the crate. Move the `SELECT COUNT(*)` in `purge_invalidated` and `revive_broken` inside the transaction so the audit count matches the actual delete. Add a ghost-anchor guard in `introduce_topic`.
**Why:** The graph layer is the foundation of the memory subsystem; non-atomic mutations leave it permanently inconsistent in ways that are hard to detect and repair. The crate already uses `unchecked_transaction()` correctly everywhere else — these are omissions, not design decisions.
**Estimated impact:** 2 CRITICAL, 4 HIGH eliminated.

### 5. Replace pervasive `resp.json().unwrap_or_default()` in CLI with a central error helper
**What:** Add a `fn check_resp(resp: reqwest::blocking::Response) -> Result<serde_json::Value, String>` to `cli/src/client.rs` that: checks HTTP status, reads body as text on error, returns `Err(formatted_message)` which `main` maps to `eprintln!` + `exit(1)`. Replace all `resp.json().unwrap_or_default()` call sites with `check_resp(resp)?`.
**Why:** Silent exit-0 on API errors is the #1 UX failure in the CLI. Users see empty output and have no way to distinguish "no data" from "daemon error". This single change fixes 13 HIGH findings.
**Estimated impact:** 13 HIGH eliminated.

### 6. Enforce mod.rs-as-facade discipline via a targeted refactor sprint
**What:** Split the 8 mod.rs files that exceed advisory limits: `mesh/mod.rs` (768 lines → `broadcast.rs`, `refresh.rs`), `plugin/mod.rs` (639 lines → `types.rs`, `manager.rs`), `config/schema/mod.rs` (751 lines → `theme.rs`, `integrations.rs`, `memory.rs`, `agent.rs`), `bridge/manager/mod.rs` (610 lines → `telegram_api.rs`), `cli/src/cmd/mem/mod.rs` (713 lines → `skill.rs`, `tail.rs`, `http.rs`). For each, keep only re-exports and top-level trait/struct declarations in `mod.rs`.
**Why:** Large mod.rs files are the primary driver of MEDIUM findings (41 of 146 MEDIUM items). They make navigation harder, tests less isolated, and CI file-size checks harder to enforce. The splits are mechanical with no behavior change.
**Estimated impact:** ~12 MEDIUM eliminated, CI file-size compliance restored.

### 7. Standardize error handling in frontend: eliminate `alert()`, unify `catch` patterns
**What:** Replace all 10 `window.alert()` calls with `addNotification` (already in the multiplexer context). Add `try/catch` to all `async` event handlers that currently have none (especially in `SecretsTab`). Standardize `listen()` setup to use the cancellation-safe pattern from `useUpdate.ts`/`useUsageSnapshot.ts`. Fix `BrowserPane.tsx` hardcoded port by accepting `api: ApiConfig` as prop.
**Why:** Three coexisting error-handling patterns and 10 UI-blocking alerts create an inconsistent user experience and mask failures. The two CRITICAL SecretsTab bugs are the direct result of no `try/catch` discipline.
**Estimated impact:** 2 CRITICAL, 2 HIGH eliminated; 9 MEDIUM reduced.

<!-- revision: 1 -->
