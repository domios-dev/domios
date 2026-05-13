# Domain Review: Startup & Commands

## Files Reviewed

### startup/
- `src-tauri/src/startup/mod.rs`
- `src-tauri/src/startup/background.rs`
- `src-tauri/src/startup/bridge.rs`
- `src-tauri/src/startup/cli_install.rs`
- `src-tauri/src/startup/cli_shim.rs`
- `src-tauri/src/startup/logging.rs`
- `src-tauri/src/startup/servers.rs`
- `src-tauri/src/startup/states.rs`
- `src-tauri/src/startup/window.rs`

### commands/
- `src-tauri/src/commands/mod.rs`
- `src-tauri/src/commands/agent.rs`
- `src-tauri/src/commands/clipboard.rs`
- `src-tauri/src/commands/completions.rs`
- `src-tauri/src/commands/cron.rs`
- `src-tauri/src/commands/hooks.rs`
- `src-tauri/src/commands/pane.rs`
- `src-tauri/src/commands/pty.rs`
- `src-tauri/src/commands/services.rs`
- `src-tauri/src/commands/skills.rs`
- `src-tauri/src/commands/system.rs`
- `src-tauri/src/commands/system_info.rs`
- `src-tauri/src/commands/tests.rs`
- `src-tauri/src/commands/update.rs`
- `src-tauri/src/commands/usage.rs`

### Top-level src-tauri/src/
- `src-tauri/src/attachments.rs`
- `src-tauri/src/autostart.rs`
- `src-tauri/src/clipboard_files.rs`
- `src-tauri/src/commands_mesh.rs`
- `src-tauri/src/keepawake.rs`
- `src-tauri/src/paths.rs`
- `src-tauri/src/process_util.rs`
- `src-tauri/src/prompts.rs`
- `src-tauri/src/shell.rs`

---

## Findings

### src-tauri/src/startup/window.rs

#### CRITICAL
- `window.rs:21` — `window.ns_window().unwrap()` can panic if `ns_window()` returns `None`. On macOS, `ns_window()` returns `Option<*mut c_void>` and the Tauri docs don't guarantee it is always `Some` (e.g., if called before the window is fully created or on a secondary webview). This is inside `setup_window_styling`, which is called from the main setup callback where the window is being initialized — low probability but still a panic path in production code.

#### MEDIUM
- `window.rs:64-67` — `read_info().expect("Failed to decode tray icon PNG")` and `reader.next_frame(&mut rgba).expect("Failed to read tray icon frame")` in `build_system_tray`. Tray icon assets are `include_bytes!`-compiled in, so failure is very unlikely, but these are `expect()` panics in non-test code. A corrupted build would crash at startup with no recovery path.

---

### src-tauri/src/startup/background.rs

#### MEDIUM
- `background.rs:46` — `tokio::runtime::Builder::new_current_thread().build().expect("Failed to create mesh health runtime")` — panics if the Tokio runtime cannot be created. Same pattern at lines 205 and 259 for the announce and update-check runtimes. Runtime creation rarely fails in practice, but an `expect()` in a background thread is a silent process crash rather than a logged error. Prefer returning/logging the error.

- `background.rs:112-113` — `std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default()` uses `unwrap_or_default()` which is safe, but the `subsec_nanos() % 5` jitter (line 223) is only 0–4 seconds. The comment says "0..5s jitter" which matches, but is misleadingly narrow for anti-spam purposes — this is advisory, not a bug.

#### LOW
- `background.rs:99-109` — Indentation inconsistency: the block starting at the `was_online` binding is indented one extra level relative to the surrounding `for peer in &peers` block. No functional issue.

- `background.rs:332` — Dead comment left in production code: `// Agent exit detection removed from idle-poll — handled in output.rs only.` This explains why something was *removed*. Belongs in a commit message, not source.

---

### src-tauri/src/startup/bridge.rs

No critical or high issues. Clean `Result<(), String>` propagation throughout.

#### LOW
- `bridge.rs:29` — `*svc.bridge_outbound_rx.lock() = None;` — sets the rx to None in the hot-restart path without explanation of what happens to any existing receiver. Presumably the old receiver is dropped (channel closes), but a brief comment would clarify intent for this non-obvious side-effect.

---

### src-tauri/src/startup/cli_install.rs

#### HIGH
- `cli_install.rs:73-88` — The install is **not atomic**. The sequence is: `fs::copy(&src, &dest)` → set permissions → create symlink. If the process is interrupted between `copy` and `set_permissions`, the destination binary exists but is not executable. On the next run, `file_sha256` comparison will see matching hashes and skip the install, leaving a non-executable binary. A safe pattern would be: copy to `dest.tmp`, set permissions on `.tmp`, then `fs::rename` (atomic on POSIX). This is the same class of issue that `keepawake.rs` correctly handles with a tmp+rename pattern.

#### LOW
- `cli_install.rs:75-77` — Re-hashing the just-written file for the log message adds an unnecessary I/O round-trip. Cosmetic.

---

### src-tauri/src/startup/cli_shim.rs

#### MEDIUM
- `cli_shim.rs:7-36` — `KNOWN_SUBCOMMANDS` is a static hardcoded list that must be kept in sync with `cli/src/args/cli.rs` manually. The test at line 161 (`detects_all_clap_top_level_subcommands`) documents this requirement but cannot enforce it automatically. When a new CLI subcommand is added without updating this list, the GUI silently swallows the invocation (the exact regression of issue #51). This is a structural DRY violation — the list lives in two places.

- `cli_shim.rs:120` — On Windows, `status.code().unwrap_or(1)` — `unwrap_or` is safe, but if the child is killed by a signal on Windows (unusual), exit code 1 is assumed. Functionally fine.

#### LOW
- `cli_shim.rs:66-69` — `is_cli_invocation` calls `arg.trim()` but `std::env::args()` already strips surrounding whitespace on all platforms. The trim is harmless but unnecessary.

---

### src-tauri/src/startup/logging.rs

Clean. No panics, no `unwrap()`. Rotation logic handles I/O errors gracefully.

#### LOW
- `logging.rs:1` — Multi-line `//!` module doc in a file that otherwise has minimal comments. Minor style inconsistency vs the one-line-max rule.

---

### src-tauri/src/startup/servers.rs

#### HIGH
- `servers.rs:95` — `std::env::set_var("CODETERM_API_PORT", port.to_string())` is called inside a `std::thread::spawn` closure that runs inside the `start_servers` function. `set_var` is not thread-safe when other threads are reading the environment concurrently (undefined behavior in the POSIX spec; Rust's std marks it `unsafe` in the 2024 edition). The env var is set after socket server startup and background task spawning, meaning there is a race window where background threads could read a stale or partially-written `CODETERM_API_PORT`. Should be set before any threads are spawned, ideally in `init_states` or at the very top of `start_servers` before the socket server start.

#### MEDIUM
- `servers.rs:61-75` — The file-sync loop thread starts immediately and sleeps for `sleep_secs` before first sync. No issue, but the `sleep_secs` floor of 300 applies even when `file_sync_interval_minutes` is 0 (the `.max(5) * 60` guards only the enabled branch). When disabled, 300s sleep per iteration is fine. When enabled with a sub-5-minute config value, the actual interval is silently floored to 5 minutes — this should arguably be surfaced in the log.

- `servers.rs:154-155` — General Agent start has a hardcoded 2-second `sleep` before calling `start_general_agent`. The comment says "Wait for frontend to initialize (workspace already created pre-lock in init_states)". This sleep is a timing guess — there's no handshake. If the frontend is slow (e.g., on a cold start with a large session restore), GA may attempt to start before the frontend is ready. This is an existing structural concern, not introduced here.

---

### src-tauri/src/startup/states.rs

#### LOW
- `states.rs:55-56` — `config_state.lock()` is called twice consecutively (once for `mesh_config`, once for `api_port`), each time acquiring and releasing the lock. Could be one lock acquisition, but parking_lot makes this cheap and the current style is consistent with the rest of the codebase.

---

### src-tauri/src/commands/completions.rs

#### CRITICAL
- `completions.rs:13` — `entry.file_type().unwrap_or_else(|_| panic!())` is an explicit `panic!()` call in a `#[tauri::command]`-annotated function. When `file_type()` returns `Err` (e.g., broken symlink, permission denied), this crashes the Tauri process entirely instead of returning `Err(String)`. This is the only explicit `panic!()` found in command handlers across all files reviewed. Should be `entry.file_type().ok()` with `map(|ft| ft.is_dir()).unwrap_or(false)`.

---

### src-tauri/src/commands/hooks.rs

#### MEDIUM
- `hooks.rs:219-236` — `hooks["Notification"].as_array_mut().unwrap()` (and same pattern for Stop, PreCompact, SessionStart). These `unwrap()` calls are safe only because the code just inserted the key with `serde_json::json!([])` two lines earlier. If the `hooks` object shape were corrupted externally, the unwrap would panic. The correctness argument is local and fragile — the `ok_or(...)` pattern would be safer and consistent with the rest of the file's error handling.

#### LOW
- `hooks.rs:11` — `CODETERM_HOOK_MARKER` is `pub(super)` but used in `commands/tests.rs` via `use super::hooks::CODETERM_HOOK_MARKER`. This pub level is correct. No issue.

- `hooks.rs:253-259` — `uninstall_claude_hooks` only removes `Notification` and `Stop` hooks, silently leaving `PreCompact` and `SessionStart` installed. The function name implies full removal. This is a consistency bug: install adds 4 hooks, uninstall removes 2.

---

### src-tauri/src/commands/pty.rs

#### MEDIUM
- `pty.rs:319-334` — `session.backend_reader_rx.lock().unwrap()` uses std `Mutex::lock().unwrap()` (not parking_lot). If the lock is poisoned (thread holding it panicked), this call panics in a `#[tauri::command]`. The same pattern appears at lines 332 and 351. These are the only uses of std `Mutex` (via `unwrap()`) in command-facing code; the rest of the codebase uses parking_lot (infallible). Inconsistency and a potential panic path.

- `pty.rs:539` — `session.pair.lock().unwrap()` in `resize_pty` — same issue, std `Mutex::lock().unwrap()` in a command handler.

- `pty.rs:243` — `let provider_id = agent_type_for_fresh.expect("checked above")` — This `expect()` is guarded by `should_fresh_launch` which requires `agent_type.is_some()`, but the variable binding at line 153 clones `agent_type` and the check is in an outer scope. The logic is correct but the `expect()` is in production command code and relies on a non-local invariant that is not enforced by the type system.

#### LOW
- `pty.rs:1-573` — This file is the largest command file at 573 lines. It mixes PTY lifecycle management (spawn, read, write, resize, kill) with agent resume logic, session auto-detection, input parsing, and cron cleanup. The `spawn_pty` function alone is ~250 lines and contains substantial business logic. Per the project's 1000-line file limit and responsibility guidelines, `spawn_pty`'s auto-resume/fresh-launch logic should be extracted to a service layer function.

---

### src-tauri/src/commands/pane.rs

#### MEDIUM
- `pane.rs:173` — `read_session_jsonl` locks `mux` once to get session data, then locks it again at line 126 (`let exclude = mux.lock().other_session_ids(&pane_id)`). These are separate lock acquisitions without any ordering concern (mux is the only lock), but the function holds a `tauri::State<'_, MuxState>` (which provides `Deref` to `Arc<Mutex<...>>`) and calls `.lock()` at two separate points — fine with parking_lot but worth noting the pattern.

---

### src-tauri/src/commands/cron.rs

#### LOW
- `cron.rs:147-148` — `cron.lock()` is called twice in rapid succession: once to `read_log`, and once to `get_history`. Both acquire and release the parking_lot lock separately. Could be one critical section, but the logic is correct and the two-lock pattern avoids holding the lock longer than necessary.

---

### src-tauri/src/commands/agent.rs

Clean. All command functions return `Result<T, String>`. No panics.

#### LOW
- `agent.rs:155` — `check_rule_installed`: `std::fs::read_to_string(&path).unwrap_or_default()` — file read failure is silently treated as empty content. For a read-only probe function this is acceptable (the function notes it "never errors"), but `unwrap_or_default()` swallows I/O errors without logging. A dev debugging a permissions issue won't see why the installed revision reads as 0.

---

### src-tauri/src/commands/system_info.rs

#### MEDIUM
- `system_info.rs:64` — `restart_app` returns `()` (not `Result<(), String>`). It is a `#[tauri::command]` but does not follow the standard `Result<T, String>` return convention. `app.restart()` does not fail visibly (it calls the platform restart), but the inconsistency is notable. Compare with all other commands that return `Result`.

- `system_info.rs:103` — `spawn_blocking` returns `unwrap_or_else(|_| ...)` — if the blocking task panics, the join error is silently converted to a fallback JSON value. This is appropriate for a diagnostics endpoint but the error is completely suppressed.

---

### src-tauri/src/commands/system.rs

#### MEDIUM
- `system.rs:22` — `test_notification` returns `()`, not `Result<(), String>`. Same convention inconsistency as `restart_app`. Both are "fire and forget" commands, but the inconsistency means the frontend cannot distinguish between "called ok" and "would have errored".

---

### src-tauri/src/commands/skills.rs

#### MEDIUM
- `skills.rs:22` — `exe.parent().unwrap_or(std::path::Path::new("."))` in `find_bundled_skills_dir` — the fallback to `"."` as exe parent means all path candidates built from `exe_dir` will be relative to the current working directory at the time of the call. For a daemon process, cwd could be anything. This is a best-effort fallback but could silently find the wrong `skills/` directory on unusual install layouts.

---

### src-tauri/src/commands_mesh.rs

#### MEDIUM
- `commands_mesh.rs:78` — `reqwest::Client::builder().build().unwrap_or_default()` — if the reqwest client builder fails (extremely rare, typically TLS backend init failure), `unwrap_or_default()` falls back to a default client. Same pattern at lines 114, 213, 244. The fallback client may not have the configured timeout, meaning network calls in these paths could hang indefinitely. Should use `build().map_err(|e| ...)` and propagate the error.

- `commands_mesh.rs:52` — `std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default()` — safe via `unwrap_or_default`.

#### LOW
- `commands_mesh.rs:381` — `use tauri::Emitter;` is imported inside the `mesh_set_access` function body even though `use tauri::Emitter` is already imported at the top of the file (line 1). This inner `use` is redundant.

---

### src-tauri/src/attachments.rs

Clean. No panics, no unwraps in non-test code. Eviction handles all error cases silently (intentional).

---

### src-tauri/src/autostart.rs

#### LOW
- `autostart.rs:79` — `dirs::home_dir().unwrap_or_default()` on macOS and Linux — if home directory cannot be found, `PathBuf::default()` (empty path) is used, causing subsequent path operations to reference the current working directory. The Windows implementation correctly errors on `current_exe()` failure but uses the same `unwrap_or_default()` pattern for path building. Should mirror the `ok_or` pattern used in other path-resolution code.

---

### src-tauri/src/clipboard_files.rs

Clean. macOS NSPasteboard correctly dispatched to main thread via `run_on_main_thread`. No panics.

---

### src-tauri/src/keepawake.rs

Clean. Atomic session ID pattern is well-designed. Snapshot uses tmp+rename correctly.

#### LOW
- `keepawake.rs:229` — `keepawake_status` returns `KeepAwakeStatus` directly (not `Result`). This is intentional (status never fails), consistent with the function's purpose, but again inconsistent with the broader command convention.

---

### src-tauri/src/paths.rs

Clean. `data_dir()` swallows `create_dir_all` errors silently — acceptable for a path helper.

---

### src-tauri/src/process_util.rs

Clean. Two-function helper, no logic.

---

### src-tauri/src/prompts.rs

Clean. All `include_str!()`, no runtime logic.

---

### src-tauri/src/shell.rs

Clean. `write_task_temp` silently ignores write errors (`let _ = std::fs::write(...)`) — if the temp file fails to write, downstream `cat`/`Get-Content` will error in the shell, which surfaces an error to the user indirectly. A log line here would aid debugging.

---

## Architecture Notes

**startup/ responsibility split** is clear and appropriate:
- `states.rs` — pure state construction, no I/O side effects beyond session restore
- `servers.rs` — server and service launch (the largest and most complex module)
- `background.rs` — long-running background threads
- `window.rs` — UI/tray setup
- `logging.rs` / `cli_install.rs` / `cli_shim.rs` — narrow single-responsibility

**commands/ business logic** — most command handlers are thin dispatchers. The main exception is `commands/pty.rs::spawn_pty`, which contains ~200 lines of session-restore and agent-launch business logic that belongs in the service layer.

**commands_mesh.rs** lives at the top level rather than inside `commands/`. This is inconsistent with the `commands/` module organization. It is not re-exported through `commands/mod.rs` but registered directly in `lib.rs`. Should be moved to `commands/mesh.rs`.

---

## Summary Table

| Severity | Count |
|----------|-------|
| CRITICAL | 2     |
| HIGH     | 2     |
| MEDIUM   | 13    |
| LOW      | 16    |
| **Total**| **33**|

### CRITICAL items (require immediate fix)
1. `commands/completions.rs:13` — explicit `panic!()` in a Tauri command handler (`list_dir_completions`)
2. `startup/window.rs:21` — `ns_window().unwrap()` in `setup_window_styling` — panics if Tauri returns `None` for the native window handle

### HIGH items
1. `startup/cli_install.rs:73-88` — non-atomic binary install: crash between `copy` and `set_permissions` leaves a non-executable binary that the hash check will permanently skip
2. `startup/servers.rs:95` — `std::env::set_var` called inside a spawned thread while other threads may be reading the environment (data race, UB in POSIX; unsafe in Rust 2024 edition)
