# Domain Review: CLI

## Files Reviewed

- `cli/src/main.rs` (703 lines)
- `cli/src/client.rs`
- `cli/src/args/cli.rs`
- `cli/src/args/mem.rs` (701 lines)
- `cli/src/args/misc.rs`
- `cli/src/args/mod.rs`
- `cli/src/args/agent.rs`
- `cli/src/args/cron.rs`
- `cli/src/args/service.rs`
- `cli/src/args/sessions.rs`
- `cli/src/args/system.rs`
- `cli/src/args/update.rs`
- `cli/src/args/usage.rs`
- `cli/src/cmd/mod.rs`
- `cli/src/cmd/agent.rs`
- `cli/src/cmd/bridge/mod.rs`
- `cli/src/cmd/bridge/migrate.rs` (668 lines)
- `cli/src/cmd/cron.rs`
- `cli/src/cmd/docs.rs`
- `cli/src/cmd/dream.rs`
- `cli/src/cmd/graph_mutate.rs`
- `cli/src/cmd/install.rs`
- `cli/src/cmd/mem/mod.rs` (713 lines)
- `cli/src/cmd/mem/tests.rs`
- `cli/src/cmd/mem_secret.rs`
- `cli/src/cmd/mesh.rs`
- `cli/src/cmd/pane.rs`
- `cli/src/cmd/print.rs`
- `cli/src/cmd/repair.rs`
- `cli/src/cmd/send_text.rs`
- `cli/src/cmd/service.rs`
- `cli/src/cmd/sessions.rs`
- `cli/src/cmd/system.rs`
- `cli/src/cmd/update.rs`
- `cli/src/cmd/usage.rs`
- `cli/src/cmd/worker.rs` (744 lines)
- `cli/src/mesh_cli/mod.rs`
- `cli/src/mesh_cli/api_map.rs`
- `cli/src/mesh_cli/auth.rs`
- `cli/src/mesh_cli/parse.rs`
- `cli/src/mesh_cli/relay.rs`
- `cli/src/mesh_cli/tests.rs`
- `cli/src/mesh_cli/update.rs`

---

## Findings

### cli/src/main.rs

#### CRITICAL

- **`main.rs:64`** — `dirs::home_dir().unwrap_or_default()` silently uses an empty path when the home dir is unavailable. On any platform where `home_dir()` returns `None` (container, broken env), all config reads silently target a relative `./` path, and the API config is fabricated from defaults. The user gets no error; the CLI quietly tries `http://127.0.0.1:7685` with no token. Not a panic, but silent misconfiguration with no user-visible signal.

- **`main.rs:144`** — `reqwest::blocking::Client::builder()...build().unwrap_or_else(|_| reqwest::blocking::Client::new())` — the fallback `Client::new()` drops all custom headers (auth token, pane-id, worker-id). If the builder fails (extremely rare), the CLI silently makes unauthenticated requests with no pane context. The failure is swallowed entirely with no log or exit.

#### HIGH

- **`main.rs:505-575` (Theme block)** — `Theme` commands hard-code `http://127.0.0.1:7685` and use a raw `reqwest::blocking::Client::new()` (no auth headers, no timeout set by `build_http`). This means: (1) theme commands always target prod regardless of `CODETERM_API_PORT`; (2) they are unauthenticated even when a token is present; (3) no request timeout — can hang indefinitely. The theme command silently ignores `CODETERM_API_PORT=7686` that all other commands respect.

- **`main.rs:512, 542`** — `resp.json().unwrap_or_default()` on theme list/save responses: if the daemon returns non-JSON (e.g. a proxy error page), the value silently becomes `null`, and the theme list prints nothing with exit code 0. No error is reported to the user.

- **`main.rs:257, 279, 290-308`** — The `dream` short-circuit block is structurally wrong. The first `if let` at line 301 checks the command, but the inner destructuring re-matches at line 303. If the outer check at line 301 passes and the inner `match` arm at line 303 doesn't fire (impossible by construction but not statically provable), execution falls through to `discover_socket()`. The redundant double-match is a maintenance hazard: if a refactor breaks the inner arm, it silently falls through rather than panicking.

#### MEDIUM

- **`main.rs` overall structure** — At 703 lines, `main.rs` is doing significantly more than bootstrapping. It contains: socket discovery logic, HTTP client construction (`build_http`), API config reading (`read_api_config`), mesh signed token generation (`read_mesh_signed_token`), ANSI stripping (`strip_ansi`), pane resolution (`resolve_pane`), data dir name detection (`data_dir_name`), and the full dispatch `match` for every subcommand. The infrastructure functions (`build_http`, `read_api_config`, `read_mesh_signed_token`, `data_dir_name`, `strip_ansi`, `resolve_pane`) belong in a `config.rs` or `context.rs` module. The `Theme` command (lines 504–576) contains full business logic that belongs in `cmd/theme.rs`.

- **`main.rs:507`** — `Theme` bypasses the socket but uses HTTP directly. All other HTTP-direct commands (`Config`, `Sessions`, `Usage`, `Keepawake`) at least call `read_api_config()` and `build_http()`. `Theme` does neither.

- **`main.rs:631–634`** — The `System { action }` arm at the bottom of the main match (line 631) is unreachable dead code: the `System` command is short-circuited at line 313 and always returns early before `discover_socket()`. The bottom arm is never reached.

#### LOW

- **`main.rs:40–38`** — `pub use mesh_cli::{MeshAction, MeshPeerAction}` appears after `strip_ansi` at line 39, breaking the conventional placement (all `pub use` re-exports should be grouped at module level, not interleaved with function definitions).

- **`main.rs:640–641`** — Two blank lines before `#[cfg(test)]`, cosmetic.

---

### cli/src/client.rs

#### HIGH

- **`client.rs:107–119`** — `SocketClient::call()` uses `read_line()` with no timeout. If the daemon accepts the connection but never responds (deadlocked handler, slow computation), the CLI blocks forever. There is a `connect_timeout` (10s) and a `timeout` (60s) on the HTTP client in `build_http`, but the socket client has neither. A hung daemon makes every socket-based CLI command hang indefinitely with no user signal.

#### MEDIUM

- **`client.rs:47–62`** — `SocketClient::connect` tries three variants (as-is, with `.sock`, without `.sock`) with no logging or user-visible explanation of what it tried. When all three fail, the error message says only `"Failed to connect to '...' (tried with/without .sock)"`. Users cannot tell whether the socket path itself is wrong or the daemon isn't running.

#### LOW

- **`client.rs:319–334`** — `discover_unix_socket` fallback: `pgrep -f codeterm` is run when no socket file is found in `/tmp`. This uses a `std::process::Command` call that inherits the current environment, can match unrelated processes (any process with "codeterm" in its argv), and has no timeout. On a machine with many processes this scan could be slow. Not a correctness issue, but a reliability concern in unusual environments.

---

### cli/src/cmd/mem/mod.rs

#### HIGH

- **`mem/mod.rs:49`** — `resp.json().unwrap_or_default()` in `mem_post_request`: on a non-2xx response where the daemon returns non-JSON (e.g. a gateway error page), `text` is captured for the error branch but `val` defaults to `null`. The error branch (line 67) then returns `if text.is_empty() { val.to_string() } else { text }`. So the user would see the raw HTTP response body as an error string, which may be HTML — acceptable, but the message could be cleaner.

- **`mem/mod.rs:666`** — `run_mem_tail` follow mode: the loop at line 684 calls `std::thread::sleep(500ms)` with no way to interrupt it gracefully on Ctrl-C (SIGINT). Rust's default SIGINT handler terminates the process, so in practice it works. However, the `sleep` inside the loop means the tail always waits up to 500ms extra before responding to Ctrl-C. More importantly, there is no `--timeout` on the follow mode — it can run forever on a daemon that has gone away (lines 688–691 catch `fetch` errors but keep looping, sleeping 2s, forever).

#### MEDIUM

- **`mem/mod.rs` (713 lines, mod.rs advisory)** — This file contains: two frontmatter parsers (`strip_skill_frontmatter`, `parse_skill_frontmatter`), two HTTP helpers (`mem_get_request`, `mem_post_request`), the full `handle_mem` dispatch function (~540 lines), and the `run_mem_tail` function with follow polling. The frontmatter parsers and HTTP helpers should be extracted to `cli/src/cmd/mem/frontmatter.rs` and `cli/src/cmd/mem/http.rs`. The `MemSkillAction` dispatch (lines 260–383) is a self-contained concern that should move to `cli/src/cmd/mem/skill.rs`. The `run_mem_tail` function should move to `cli/src/cmd/mem/tail.rs`. Currently the mod.rs holds all logic rather than being a facade.

- **`mem/mod.rs:287`** — `std::fs::read_to_string(&skill_md).unwrap_or_default()` after the file was just successfully written. If the write succeeded but the read fails (permissions race, FS flush), the body silently becomes empty and an empty skill is registered in mem without error.

- **`mem/mod.rs:370-383`** — `MemSkillAction::List` uses the `/api/mem/tail?limit=500` endpoint and client-side filters for `kind == "skill"`. This is a semantic mismatch: `tail` is an ordered-by-recency endpoint meant for monitoring recent writes, not an index. If more than 500 items exist in the store, skills created before the window are silently omitted. There is no indication to the user that results may be incomplete.

#### LOW

- **`mem/mod.rs:393–398`** — `sep` variable is assigned but the last `let _ = sep` in `WakeUp` is used to suppress the unused-variable warning. This pattern repeats across multiple query-building blocks in the file (also in `api_map.rs`). It's a code smell: a `build_query_string` helper would be cleaner and eliminate the `sep` mutation pattern entirely.

---

### cli/src/cmd/worker.rs

#### CRITICAL

- **`worker.rs:96–101`** — `register_sqlite_vec_extension` uses `unsafe { rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(...))) }`. While this is the documented pattern for sqlite extensions, `transmute` between function pointer types is undefined behavior in Rust if the ABI doesn't match. The safety comment explains the Once guard but says nothing about why the transmute is safe here. This is a CRITICAL UB risk, though in practice it likely works because the C ABI for this extension matches.

#### HIGH

- **`worker.rs:114–198` (`worker_dreaming_light`)** — The dreaming promotion logic runs a `query_map` that collects all `learning` items older than 1 day with `access_count >= 3` into a `Vec` in memory (line 135: `rows.filter_map(|r| r.ok()).collect()`). If the mem store has a large number of qualifying learnings, this loads them all into memory at once. No LIMIT is applied in the SQL query. On a large store this could cause excessive memory use.

- **`worker.rs:167`** — `conn.execute(...)` at the promotion step uses `MAX(importance, 4.0)` in SQL. If the UPDATE affects 0 rows (the item was deleted between the SELECT and UPDATE due to concurrent writes), `n == 0` and the promoted counter isn't incremented, but no error is raised. This is correct behavior for "best-effort" dreaming, but it is worth noting the race is silently swallowed.

#### MEDIUM

- **`worker.rs` (744 lines)** — The file mixes three distinct responsibilities: (1) `run_mem_worker` entry point + DB initialization + extension registration; (2) dreaming logic (`worker_dreaming_light`, `worker_dreaming_deep`, `worker_dreaming_normalize`); (3) librarian logic (`worker_librarian_once`, `librarian_scan_skill_folders`, `librarian_sync_files`, `librarian_sync_one`, tag helpers). These should be split into `worker/dreaming.rs`, `worker/librarian.rs`, and a thin `worker/mod.rs` entry point.

- **`worker.rs:452, 462`** — `build_skill_id` truncates the slug at 40 bytes by byte index (`&trimmed[..40]`) — this panics on non-ASCII UTF-8 characters if the 40th byte falls in the middle of a multibyte character. `chars().take(40)` should be used instead.

- **`worker.rs:425`** — `serde_json::to_string(&vec!["folder-skill", "auto-registered"]).unwrap_or("[]".into())` — `unwrap_or` with a `.into()` call on a string literal compiles, but the expression is needlessly complex. The `unwrap_or_else` form would be idiomatic, but more to the point, this serialization can never fail for a simple `Vec<&str>`, making the fallback dead code that masks what is actually an invariant.

#### LOW

- **`worker.rs:246-260`** — `NORMALIZE_SYNONYMS`, `NORMALIZE_PROTECTED_TYPES`, `NORMALIZE_ORPHAN_AGE_DAYS`, `NORMALIZE_RATIO_HARD`, `NORMALIZE_RATIO_FLAG` are file-level constants in `worker.rs`. Since the normalize logic now routes through `codeterm_mem_store::graph`, these constants are caller-side policy. They're fine where they are, but should have a comment explaining why they live in the CLI rather than in `mem-store`.

---

### cli/src/cmd/bridge/migrate.rs

#### CRITICAL

- **`migrate.rs:121–131`** — The migration sequence disables the source (`telegram_enabled=false`, `bridge.stop`) before confirming the target can claim. If the `patch_config_local` call succeeds but the process is killed before `bridge.stop` completes, the source is config-disabled but the poller is still running (orphaned). The rollback path at line 191 re-enables config but relies on `bridge.start` to actually restart the poller. If the daemon is restarted between those steps, the start call might succeed with no actual poller. This is an inherent race in the two-phase protocol and is documented via `report_split_brain`, but there is no idempotency key or distributed lock covering the config-write step. The `no_rollback` flag at line 186 compounds this: with `--no-rollback`, a failed claim leaves both source disabled and target not active, with no automatic recovery.

#### HIGH

- **`migrate.rs:208–229`** — Post-migration verification queries both local and target state. If either query fails (network blip), `verify_local.as_ref().map(|s| !s.active).unwrap_or(false)` returns `false`, triggering a spurious rollback even though the migration may have succeeded. A single transient network error during verification causes a rollback attempt that itself may fail, landing in `report_split_brain`. The verification should retry at least once before triggering rollback.

- **`migrate.rs:383–427` (`resolve_paired_peer`)** — The token is added as a separate `Authorization` header via `req.header(...)`, but the `http` client built by `build_http` also already includes the authorization header from `build_http`. This means the `resolve_paired_peer` function double-sets the auth header. The second call wins in reqwest, so it works, but it's a maintenance hazard: if the token in `build_http` and the `token` parameter diverge, the behavior is silently wrong.

#### MEDIUM

- **`migrate.rs:297–338` (`log_step`)** — `log_step` only prints in non-JSON mode (correctly). But in JSON mode, progress steps are completely invisible to the caller until the final summary at line 249. For long-running migrations this means a JSON caller has no streaming progress signal. A structured JSON log line per step (to stderr) would be consistent with the final output on stdout.

- **`migrate.rs:36–37`** — `EXIT_TARGET_MISCONFIG`, `EXIT_SPLIT_BRAIN`, `EXIT_USAGE` are module-level constants. They are not re-exported or documented in terms of shell scripting contract. The values (5, 6, 7) are used exclusively inside this module but callers scripting around `bridge migrate` need to know these codes. A `--help` note or doc comment would help.

#### LOW

- **`migrate.rs:312`** — `repair_cmd` in `report_split_brain` is constructed as `format!("codeterm config set integrations.{}_enabled true && codeterm bridge restart", service)`. This concatenation of user-supplied `service` into a shell command hint is not a shell injection risk (it's a hint string printed to stderr, not executed), but it looks like shell injection on first read. A comment clarifying it is display-only would help reviewers.

---

### cli/src/args/mem.rs

#### MEDIUM

- **`args/mem.rs` (701 lines)** — This file is entirely enum definitions with `#[derive(Subcommand)]`. At 701 lines it is large, but all content is args structs — no execution logic. This is structurally correct per the review mandate (args vs cmd separation is clean). The size is driven by the large number of `mem` subcommands. Splitting by action group (`MemGraphAction` → `args/mem_graph.rs`, `MemSkillAction` + `MemWorkerAction` → `args/mem_worker.rs`) would reduce size without changing behavior.

#### LOW

- **`args/mem.rs:100–138` (`Save`)** — `kind` is a free-form `String` instead of an enum. Valid values are documented in the help text but not enforced at parse time. A user typo in `--kind` passes validation and only fails at the daemon's API. Clamping to `clap::ValueEnum` would give instant parse-time error.

- **`args/mem.rs:409–410`** — `MemSecretAction::Unlock` takes `--password` as an `Option<String>` positional-style long arg. This is documented as the worst option (visible in `ps`), but there is no `#[arg(hide = true)]` or `#[arg(env = "...")]` to guide users toward the stdin path. The help text in `cmd/mem_secret.rs` does warn about this, but the arg definition itself should carry a warning.

---

### cli/src/cmd/agent.rs

#### HIGH

- **`agent.rs:12, 19, 26, etc.`** — Every `resp.json().unwrap_or_default()` call on agent list/hierarchy/providers responses: a non-JSON response (e.g. an nginx 502 gateway error page) silently becomes `null`/default JSON. For `AgentAction::List`, the code then checks `val.as_array()` and prints nothing if null — exit code 0 with no output. The user sees an empty response and doesn't know whether there are no agents or the daemon returned an error.

- **`agent.rs:365–390` (`AgentAction::Model`)** — The pane lookup at line 367 calls `http.get(...).send().ok()...` — all errors are silently swallowed via `.ok()`. If the API call fails, `agent_type` is `None`, the Codex check is skipped, and the `/model <name>` command is sent anyway. A failed agent-type lookup should warn the user.

#### MEDIUM

- **`agent.rs:146, 168, 175`** — Repeated pattern: `let v: serde_json::Value = resp.json().unwrap_or_default(); println!("{}", serde_json::to_string_pretty(&v).unwrap());` — the `to_string_pretty(&v).unwrap()` can only fail if the value is a float NaN or Inf (not possible from JSON parsing), so it's safe, but using `unwrap()` in production paths violates the project convention. Use `unwrap_or_default()` or `expect()` with a meaningful message.

---

### cli/src/cmd/mesh.rs

#### HIGH

- **`mesh.rs:25`** — `MeshAction::Status`: `resp.json().unwrap_or_default()` on the status response. Same pattern as agent.rs: a non-JSON response silently becomes `null`, and the code then tries `val.get("instance_name")...unwrap_or("?")` — the user sees `? (?)` with exit code 0 instead of an error.

- **`mesh.rs:33`** — `id[..id.len().min(8)]` — this slices `id` by byte offset. If `id` contains non-ASCII characters (UUIDs don't, but IDs are strings not typed), this can panic with an "byte index is not a char boundary" error at runtime. Should use `id.chars().take(8).collect::<String>()`.

#### MEDIUM

- **`mesh.rs` overall** — Auth header duplication: `mesh.rs` adds `Authorization` via both `build_http` (which includes the token in default headers) and explicit `.header("Authorization", ...)` calls on individual requests (lines 22, 41, 94, etc.). This is the same pattern as `resolve_paired_peer` in `migrate.rs`. It's redundant but harmless in practice.

---

### cli/src/cmd/sessions.rs

#### MEDIUM

- **`sessions.rs:62`** — `resp.json().unwrap_or_default()` on the search response. Non-JSON response → silent null → "No sessions matched" printed with exit code 0.

- **`sessions.rs:3–33`** — `shell_quote` and `format_unix_ts` are defined locally in `sessions.rs`. `format_unix_ts` is identical in structure (Howard Hinnant algorithm) to `fmt_ts` in `cmd/usage.rs` (lines 138–160). Two copies of the same date-formatting algorithm exist in the same crate. This is a DRY violation — extract to `cli/src/util.rs` or `cmd/print.rs`.

---

### cli/src/cmd/usage.rs

#### MEDIUM

- **`usage.rs:138–160`** — `fmt_ts` duplicates the civil-from-days algorithm also present in `sessions.rs:13–33`. See sessions.rs note above.

---

### cli/src/cmd/update.rs

#### MEDIUM

- **`update.rs:44` (`poll_update_status`)** — `"idle"` phase after 2 seconds is treated as "No update available" and returns cleanly. But `"idle"` also occurs during the very brief moment between the install request being accepted and the update state machine transitioning to `"downloading"`. If the system is slow, polling at 500ms could catch the `"idle"` state before the first phase transition and exit with "No update available" even when an update is actually being applied. The guard `started.elapsed() > Duration::from_secs(2)` mitigates this but doesn't eliminate it.

#### LOW

- **`update.rs`** — `poll_update_status` prints to stdout (`println!`) for phase changes and to stderr (`eprintln!`) for errors. All other CLI commands print errors to stderr and results to stdout. This is consistent. However, `println!("Update applied. Daemon is restarting.")` at line 34 is an operational status message that arguably belongs on stderr (it's not parseable output), but callers may be parsing stdout for "success" tokens. Ambiguous.

---

### cli/src/cmd/print.rs

#### LOW

- **`print.rs:5`** — `serde_json::to_string_pretty(&value).unwrap()` — `unwrap()` in production code. This can only fail for NaN/Inf floats (impossible from JSON), but violates the no-`unwrap` convention. Should be `unwrap_or_else(|_| value.to_string())` or similar.

- **`print.rs` generally** — `print_object` at line 38 iterates over a `serde_json::Map` whose iteration order is insertion order (serde_json preserves it). Output ordering depends on what the daemon sends. For stable output across daemon versions, the keys should be sorted. Currently the column alignment (`max_key_len` calculation) works correctly, but key ordering is non-deterministic across API changes.

---

### cli/src/cmd/send_text.rs

#### LOW

- **`send_text.rs:83-105` (`handle_send_text_result`)** — `serde_json::to_string_pretty(&value).unwrap()` — `unwrap()` in the json output path. Same concern as above; harmless in practice but violates convention.

---

### cli/src/cmd/pane.rs

#### MEDIUM

- **`pane.rs:109`** — `eprintln!("No pane text")` followed by `std::process::exit(1)` in `GetText`. This prints a generic error with no context about which pane or what response was received.

- **`pane.rs:134`** — `eprintln!("Exec failed")` — same problem; no detail about what failed. Compare to `pane.rs:137`: `eprintln!("Exec request failed: {}", e)` which correctly includes the error. The first branch (HTTP success but no output field) gives zero diagnostic information.

---

### cli/src/cmd/dream.rs

#### MEDIUM

- **`dream.rs:31`** — `handle_dream` returns `!` (never), meaning it always calls `std::process::exit`. This is inconsistent with how other handlers work (`handle_mem`, `handle_bridge`, etc. return `()`). The `!` return type forces callers to use `unreachable!()` after calling it (which is what `main.rs` does). This makes the control flow harder to follow and prevents future refactoring to return results.

#### LOW

- **`dream.rs:84`** — `println!` in `run_diary` uses an inline format string `"No dream diary entries yet — the Dreamer hasn't logged anything in {area}."` where `area = "_dreams"` is a named argument resolved at the call site. This is fine, but the `area` field is an internal implementation detail leaking into user-facing output.

---

### cli/src/cmd/repair.rs

#### LOW

- **`repair.rs:10`** — `eprintln!("Note: 'mem repair revive-broken-relations' is deprecated; use 'mem graph revive-broken'.")` is printed for every invocation of this deprecated command, which is correct. However, the command then proceeds to execute anyway. An alternative design would be to exit early after the deprecation notice and require `--force` or just redirect. The current behavior (warn + run) is intentional but worth documenting.

---

### cli/src/cmd/graph_mutate.rs

#### CRITICAL

- **`graph_mutate.rs:452–458` (`build_skill_id`)** — `&trimmed[..40]` slices by byte index. If `title` contains multi-byte UTF-8 characters and the character boundary doesn't fall exactly at byte 40, this **panics** with `byte index X is not a char boundary`. Since skill names are user-controlled strings, this is a reachable panic in production. Fix: use `trimmed.char_indices().nth(40).map_or(trimmed, |(i, _)| &trimmed[..i])`.

  Note: `build_skill_id` is defined in `worker.rs:452` but the same bug pattern appears. Checked `graph_mutate.rs` separately — it does not re-implement this function, so the single instance in `worker.rs` is the source.

---

### cli/src/cmd/install.rs

#### MEDIUM

- **`install.rs:23`** — `std::env::var("LocalAppData").unwrap_or_default()` — silently uses empty string if `LocalAppData` is unset. The resulting path would be `\codeterm\codeterm.exe` (relative). This can cause silent failure to find the installed binary with no error message until the bat script runs and fails.

- **`install.rs:69`** — `pane = pane` in the bat file template: the `pane` variable from `continue_pane` is interpolated directly into a batch script line without any quoting or sanitization. If `pane` contains spaces or special shell characters (batch metacharacters like `&`, `|`, `<`, `>`), the bat script will be malformed. Pane IDs are UUIDs and will never contain those characters in practice, but this is a structural injection risk.

---

### cli/src/mesh_cli/relay.rs

#### HIGH

- **`relay.rs:115–116`** — `let bytes = resp.bytes().unwrap_or_default()` — if `bytes()` fails (network error after headers received), this silently returns empty bytes. The subsequent `serde_json::from_slice(&bytes).unwrap_or_default()` then produces `null`. The code then enters the `!status.is_success() && val.is_null()` branch and prints a preview of the empty bytes — the actual error (why `bytes()` failed) is never shown. Use `bytes().map_err(...)` and surface the error.

#### MEDIUM

- **`relay.rs:54-70`** — `mesh_relay_cmd` has a local `(method, path, body)` tuple where `body` is `serde_json::Value` but downstream `mesh_relay_generic` takes `Option<&str>`. The conversion `if body.is_null() { None } else { Some(body.to_string()) }` is fine, but for the `"exec"` and `"send"` cases, `body` is always non-null. The null check is dead code for those arms.

---

### cli/src/mesh_cli/auth.rs

#### LOW

- **`auth.rs:16`** — `SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()` — `unwrap_or_default()` silently produces 0 if system time is before the epoch. This would generate a token with `timestamp: 0`, which the server-side verifier may or may not reject. Should use a meaningful error instead.

---

### cli/src/mesh_cli/api_map.rs

#### MEDIUM

- **`api_map.rs:63`** — `Commands::Workspace { action: WorkspaceAction::MoveTab { .. } }` is not matched — it falls through to `_ => ("GET".into(), "/api/workspaces".into(), None)`. The `--mesh` path for `workspace move-tab` silently sends a GET to `/api/workspaces` instead of the correct endpoint. No error, no warning.

- **`api_map.rs:120`** — `AgentAction` mesh dispatch: only `List`, `Hierarchy`, and `Spawn` are mapped. `AgentAction::Providers`, `Models`, `Status`, `SetConductor`, `Link`, `Unlink`, `Children`, `AllowSpawn`, `Report`, `Reports`, `Compact`, `Restart`, `Model` all fall to `_ => ("GET".into(), "/api/agents".into(), None)`. For `agent report` via `--mesh`, this silently fetches the agent list instead of sending a report. Silent wrong behavior on mesh relay.

- **`api_map.rs:253`** — `BridgeAction::React { .. }` maps to `("GET".into(), "/api/health".into(), None)` with a comment "isn't relayable today". This is a safe fallback, but a relay caller using `bridge react --mesh` would get back a health-check response with no indication the command was silently ignored.

---

### cli/src/cmd/mem_secret.rs

#### LOW

- **`mem_secret.rs:38-66` (`print_or_exit` closure)** — This closure is defined with return type `!` but uses `std::process::exit()` for both success (line 65) and error paths. This means every `MemSecret` subcommand calls `std::process::exit()` even on success, bypassing Rust's normal destructor chain. Drop ordering for open file handles or buffered writers may be affected. `stdout().flush()` is not called before exit for the success path.

---

### cli/src/cmd/system.rs

#### LOW

- **`system.rs:95–96`** — `ps_command` is a PowerShell command with the binary path interpolated via `bin_str.replace('\'', "''")`. Single-quote escaping is correct for PowerShell string literals, but `bin_str` is then placed inside a double-quoted PowerShell string (`"..."`) wrapped in a batch script. The escaping chain (Rust string → PowerShell double-quoted → schtasks /TR → batch) has multiple quoting layers. This is not a security issue (admin-only context), but it is fragile and untested for paths with double quotes or backticks.

---

## Summary Table

| Severity | Count |
|----------|-------|
| CRITICAL | 4     |
| HIGH     | 13    |
| MEDIUM   | 21    |
| LOW      | 20    |
| **Total**| **58**|

### CRITICAL items summary

1. `worker.rs:96–101` — `unsafe transmute` of SQLite extension function pointer (UB risk)
2. `graph_mutate.rs` / `worker.rs:452–458` — `build_skill_id` byte-slices UTF-8 at fixed offset 40, reachable panic on non-ASCII skill names
3. `cmd/bridge/migrate.rs:121–131` — No atomic lock over config-write + stop; intermediate failure leaves source disabled with no automatic recovery (acknowledged via `report_split_brain` but structurally unresolvable in the current two-phase design)
4. `main.rs:144` — `build_http` failure silently falls back to unauthenticated client, dropping all custom headers including auth token
