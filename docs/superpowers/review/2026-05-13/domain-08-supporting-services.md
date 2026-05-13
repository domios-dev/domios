# Domain Review: Supporting Services

## Files Reviewed

- `src-tauri/src/services/activity.rs`
- `src-tauri/src/services/mod.rs`
- `src-tauri/src/services/push.rs`
- `src-tauri/src/services/smart_add.rs`
- `src-tauri/src/services/tokens.rs`
- `src-tauri/src/services/health/mod.rs`
- `src-tauri/src/services/health/http.rs`
- `src-tauri/src/services/health/process.rs`
- `src-tauri/src/services/health/scheduler.rs`
- `src-tauri/src/services/health/script.rs`
- `src-tauri/src/services/health/tcp.rs`
- `src-tauri/src/services/store/mod.rs`
- `src-tauri/src/services/store/crud.rs`
- `src-tauri/src/services/store/cron_view.rs`
- `src-tauri/src/services/store/schema.rs`
- `src-tauri/src/services/store/tests.rs`
- `src-tauri/src/services/store/types.rs`
- `src-tauri/src/session/mod.rs`
- `src-tauri/src/session/store.rs`
- `src-tauri/src/update/apply.rs`
- `src-tauri/src/update/channel.rs`
- `src-tauri/src/update/checker.rs`
- `src-tauri/src/update/manifest.rs`
- `src-tauri/src/update/mod.rs`
- `src-tauri/src/update/post_update.rs`
- `src-tauri/src/update/shutdown.rs`
- `src-tauri/src/usage/mod.rs`
- `src-tauri/src/usage/poller.rs`
- `src-tauri/src/usage/probe.rs`
- `src-tauri/src/usage/projection.rs`
- `src-tauri/src/usage/projection_tests.rs`
- `src-tauri/src/usage/store.rs`
- `src-tauri/src/usage/schema.sql`

---

## Findings

### src-tauri/src/services/store/crud.rs

#### HIGH
- **`crud.rs:166,169` — SQL injection via string formatting in `list()`**
  `kind` and `enabled` filter values are interpolated directly into the SQL string using `format!()` rather than bound parameters. `kind_to_str()` is exhaustive over a closed enum so there is no injection vector today, but if `ServiceKind` gains a variant with special characters before this code is updated, the string will land unescaped in the query. The `enabled` branch formats a bare integer literal which is safe, but the pattern itself is fragile — both filters should use `rusqlite::params!` like the rest of the codebase.

#### MEDIUM
- **`crud.rs:112` — `unwrap()` on `serde_json::to_string()` in `create()`**
  `serde_json::to_string(m).unwrap()` inside `params![...]` for `field_mapping`. `FieldMapping` wraps a `serde_json::Value` which cannot fail to serialize, so this will not panic in practice, but it is an unconditional `unwrap` in production code.

- **`crud.rs:243` — same `unwrap()` in `update()`**
  Same pattern for `field_mapping` in the update path.

- **`crud.rs:177-258` — update() fires one SQL statement per changed field**
  Each non-None field in `ServiceUpdate` issues a separate `UPDATE` with its own round-trip and `updated_at` bump. If two fields change simultaneously they produce two `updated_at` timestamps (which may differ by a nanosecond) and the last write wins. No transaction wraps the multi-field update — a crash mid-update leaves the record partially patched. A single `UPDATE services SET f1=v1, f2=v2, … WHERE id=?` would be atomic and cheaper.

- **`crud.rs:302-303` — `row.get(18..21).ok()` silently ignores column errors**
  The `service_health` LEFT JOIN columns (index 18–21) use `.ok()` to swallow any column-read error. A schema mismatch on those columns would silently produce `None` health data with no diagnostic.

### src-tauri/src/services/health/scheduler.rs

#### MEDIUM
- **`scheduler.rs:25-33` — no shutdown mechanism**
  `start()` spawns an infinite `loop` with no cancellation token, shutdown channel, or `JoinHandle`. The task runs until the process exits; there is no way to stop or restart the scheduler without restarting the daemon. Comment in `CLAUDE.md` ("subsystems should support stop/start without restarting the whole daemon") is violated.

- **`scheduler.rs:50-57` — probe result is always persisted regardless of debounce**
  `persist_snapshot` is called on every tick for every service, including while a status transition is still in the debounce window. This means the DB receives every transient probe result; the debounce only governs event emission and activity logging, not persistence. The per-service health history can grow with noisy down/up flickers without any of them surfacing in the UI. Likely intentional for history fidelity, but the comment is absent.

- **`scheduler.rs:88-115` — `probe_one` is called sequentially for all services**
  HTTP, TCP, and script probes all await inline inside `run_tick`. With 10+ services, a 5-second HTTP timeout per service means a single tick can block for minutes. No concurrency or timeout budget governs the full tick.

#### LOW
- **`scheduler.rs:12` — `TICK_SECS = 30` is the global interval regardless of per-service `health_interval`**
  `Service.health_interval` is stored in the DB but not consulted by the scheduler, which probes every service every 30 seconds unconditionally. The field has no effect.

### src-tauri/src/services/health/http.rs

#### LOW
- **`http.rs:24` — `resp.text().await.unwrap_or_default()`**
  If reading the body fails (e.g. connection reset after headers), this silently falls back to an empty string, which `serde_json::from_str` then parses as `{}`. The resulting `status_str` is `"ok"` → `HealthStatus::Healthy`, so a truncated response with a successful status code is treated as healthy. This is a corner case (the HTTP status already passed), but the silent discard is worth noting.

### src-tauri/src/services/health/process.rs

#### MEDIUM
- **`process.rs:14-28` (macOS) — `launchctl list <label>` succeeds for any listed service**
  The macOS probe runs `launchctl list <label>` and checks only that the command succeeded and stdout is non-empty. It does not inspect the `"LastExitStatus"` field in the output JSON. A service that crashed but is registered in launchd (exit status -1) will still be reported `Healthy`. The Linux probe (`is-active`) correctly checks the string value.

### src-tauri/src/services/tokens.rs

#### LOW
- **`tokens.rs:50-61` — `lookup_service_id()` is O(n) scan**
  Every push validation that goes through `validate_push` → `extract_service_token` → `lookup_service_id` scans all service token files on disk to find a match. With many services this is a linear scan; with slow disk it blocks the calling thread. No in-memory index exists. Not critical at current scale, but worth noting since this is on the hot path for push ingestion.

### src-tauri/src/services/smart_add.rs

#### LOW
- **`smart_add.rs:19-20` — `unwrap_or_default()` silently discards HTTP body read errors**
  `response.text().await.unwrap_or_default()` and `serde_json::from_str(&body).unwrap_or(…)` silently drop errors. The caller receives a structurally empty `SmartAddResult` with no indication of why the inspection failed. Since this is a best-effort UI helper, the impact is low, but the error should propagate or at least be logged.

- **`smart_add.rs:35-36` — always returns `recommended_health_path: Some("/healthz")`**
  Even when the inspected URL responded on a different path, the recommended path is always `/healthz`. This is a placeholder default that ignores the actual URL probed.

---

### src-tauri/src/session/store.rs

#### MEDIUM
- **`store.rs:200-205` — `save_named()` is not atomic**
  `save_named` writes directly with `fs::write()` (not via a tmp-then-rename sequence). A crash during the write corrupts the named session file with no recovery path. The comment on `save()` at line 95-101 explicitly explains why direct `fs::write` is unsafe; `save_named` does not apply the same discipline.

- **`store.rs:207-214` — `load_named()` does not fall back to backups**
  If a named session file is corrupt or partially written, `load_named` returns an error with no fallback. Since `save_named` is not atomic (above), this creates a window where `load_named` fails with no recovery.

#### LOW
- **`store.rs:88-102` — `save()` loses the previous last.json if `rotate_backup` fails**
  The code calls `rotate_backup` before writing the new file. If `rotate_backup` fails (e.g. full disk), the save proceeds anyway — the old `last.json` is not backed up but the new one is written. The old snapshot is silently lost. The `let _ = self.rotate_backup(&path)` intentionally ignores errors; a log message would aid diagnosis.

- **`store.rs:109-113` — backup filename collision on same-second saves**
  Backups are named `{unix_seconds}.json`. Two saves within the same second overwrite each other's backup. The test suite calls `store.save()` back-to-back in the same millisecond and the second `fs::copy` silently overwrites the first backup file. In practice this is harmless since `last.json` is the source of truth, but the backup ring can under-count actual snapshots.

---

### src-tauri/src/update/apply.rs

#### HIGH
- **`apply.rs:83-96` — phase set to `Applying` after download, but binary is already swapped**
  `tauri-plugin-updater`'s `download_and_install()` performs the binary swap synchronously before returning. The phase is set to `Downloading` before the call and only to `Applying` after it succeeds. If the process crashes between `download_and_install` returning `Ok` and `set_phase(Applying)` being called, the new binary is installed but the state machine never recorded the completion. On the next boot the updater would consider itself in the `Applying` phase from the stale `UpdateState`, except `UpdateState` is in-memory and resets to `Idle` on restart. Net effect: the inconsistency is self-healing (because state is transient), but it means no persistent record of a successful apply is written anywhere. If the restart itself fails (new binary crashes on startup), there is no persistent flag to detect a boot loop.

- **`apply.rs:83` — `download_and_install` errors are fatal with no cleanup**
  If `download_and_install` returns `Err`, the function returns `Err(msg)`. A partial download or partial binary overwrite at this point is handled entirely by `tauri-plugin-updater`; there is no application-level fallback or rollback. Whether the plugin's own recovery is safe is opaque from this layer.

#### MEDIUM
- **`apply.rs:21-24` — only `default_endpoints(opts.channel)[0]` is used**
  `default_endpoints` returns a `Vec<String>` implying multiple fallback endpoints, but only index `[0]` is consumed. The vector API suggests future-proofing that isn't wired. The Tauri updater builder is only given the overridden endpoint if `manifest_url` is `Some`; when it's `None`, the plugin uses whatever was compiled in (from `tauri.conf.json`), not the channel-derived endpoint. This means the `channel` field on `ApplyOptions` has no effect unless `manifest_url` is explicitly set by the caller.

### src-tauri/src/update/checker.rs

#### LOW
- **`checker.rs:49` — `strip_prefix('v')` with `unwrap_or(tag)` is silently lenient**
  If the GitHub release tag has no `v` prefix (e.g. `1.2.3` instead of `v1.2.3`), `strip_prefix` returns `None` and `unwrap_or` falls through to the raw tag. This is fine, but an unexpected tag format like `release-1.2.3` would survive to `semver::Version::parse` and produce a clear error, so the fallback is harmless.

- **`checker.rs:66-67` — `release_url` defaults to `""` on missing `html_url`**
  `resp["html_url"].as_str().unwrap_or("").to_string()` produces an empty URL rather than an error. The caller receives `UpdateInfo` with `release_url = ""`, and the UI would link to nothing. Should return `Err` or at minimum log the anomaly.

---

### src-tauri/src/usage/store.rs

#### MEDIUM
- **`store.rs:141-146` — `get_history` silently drops row-level deserialization errors**
  `.filter_map(Result::ok)` on `query_map` results silently discards any row that fails to deserialize. A schema version mismatch or column type change would silently produce a shorter history than expected, with no log or error. Same pattern in `get_daily_rollup` (line 171-174).

- **`store.rs:117-118` — `get_latest` uses `format!()` to build SQL**
  ```rust
  let sql = format!("{} WHERE provider = ?1 AND (?2 IS NULL OR account_id = ?2) AND parsed_ok = 1 ORDER BY captured_at DESC LIMIT 1", SELECT_HEADER);
  ```
  Not an injection risk (no user data interpolated), but the same construction with a bare `const` string would be cleaner and avoids the runtime allocation on every call to `get_latest`. Same in `get_history` (line 137).

### src-tauri/src/usage/projection.rs

#### CRITICAL
- **`projection.rs:231` — `remaining_ms as f64` implicit cast on potentially negative value**
  ```rust
  let remaining_ms = (resets_at - now_ms).max(0) as f64;
  ```
  The `.max(0)` guard makes the cast safe here. However, at line 222 `dt_ms` is computed as `last.0 - first.0` with no guard before being used as a divisor (`dt_ms as f64` then `d_pct / dt_ms as f64`). If by any DB anomaly `last.0 < first.0` (rows out of order despite `ORDER BY captured_at ASC`), `dt_ms` is negative, `rate_per_ms` is negative, and `projected` is less than `current_pct`. The check at line 229 (`if rate_per_ms <= 0.0 { return Ok(None) }`) catches the negative case for the projection result, but only after the negative rate is computed. This is safe in outcome but fragile — adding an explicit guard `if dt_ms <= 0 { return Ok(None) }` at line 223 would be clearer.

#### HIGH
- **`projection.rs:513-514` — `BASELINE_CACHE` is a process-global `OnceLock<Mutex<…>>`**
  The cache is keyed by `(provider, account_id)` and has a TTL but is never bounded. In principle, with many provider+account combinations it grows without limit. In practice only one provider is active, but if the key space grows (multi-account) the cache becomes a memory leak.

- **`projection.rs:513-515` — `cache().lock()` uses `std::sync::Mutex`, not `parking_lot`**
  The rest of the codebase uses `parking_lot::Mutex` which is non-poisoning. The projection cache uses `std::sync::Mutex` (via `use std::sync::{Mutex, OnceLock}`). If any thread panics while holding the projection cache lock, all future calls to `baseline_for()` silently fall back to recomputing the baseline on every call (because `if let Ok(guard) = cache().lock()` fails) — a degraded but non-crashing outcome. However, the inconsistency with the rest of the codebase is a hazard: a future maintainer may add `unwrap()` assuming the parking_lot guarantee.

- **`projection.rs:758-765` — `local_start_of_day_ms` uses `.single().unwrap_or(dt)`**
  During the DST "spring forward" hour (e.g. 2:00 AM doesn't exist), `Local.with_ymd_and_hms(..., 0, 0, 0).single()` returns `None` if midnight is in a DST gap, and the fallback is `dt` (the original input timestamp). This means `local_start_of_day_ms(now_ms)` returns `now_ms` itself during the DST gap, which would cause `today_start_ms > now_ms` for some inputs and could break the `captured_at >= today_start_ms AND captured_at <= now_ms` queries to return zero rows. The `compute_weekly` and `today_delta` functions would silently return zero-delta results during DST transitions.

#### MEDIUM
- **`projection.rs:339` — cycle-day loop uses `cursor += DAY_MS` (fixed 86 400 000 ms)**
  Walking future days by adding 86 400 000 ms (exactly 24 h) crosses DST boundaries incorrectly. On a "fall back" night the 25th hour is not counted; on a "spring forward" night the cursor skips ahead by 23 h and may double-count or skip a day at the boundary. This affects `end_of_cycle_pct` when the billing cycle end spans a DST transition.

- **`projection.rs:277` — `share_used` clamped to `[0.0, 1.0]` but `shape` values are not normalized to sum=1 in all paths**
  The code clamps `share_used` at 1.0, but `wd_hour_share` / `we_hour_share` are only guaranteed to sum to 1.0 when computed from `wd_share_n >= 2` days of data. When the default profile is used (`wd_share_n < 2`), `default_wd_share` is normalized from `DEFAULT_HOURLY_WEEKDAY` whose sum is 12.2 — so `default_wd_share` does sum to 1.0. Verified safe, but no comment or assertion makes this invariant explicit.

- **`projection.rs:769` — file is 769 lines; extractable concerns**
  Three distinct logical units are interleaved: (1) DB query helpers (`compute_session`, `today_delta`, `latest_weekly`, `recent_rate`), (2) statistical model (`compute_baseline`, `median`), and (3) projection assembly (`compute_weekly`). Splitting into `projection/model.rs` and `projection/queries.rs` would keep each under 300 lines and match the style guide.

#### LOW
- **`projection.rs:674` — inner `fn median()` defined inside `compute_baseline`**
  `median` is a pure free function but is defined nested inside `compute_baseline`. It cannot be unit-tested directly. Moving it to module scope (or the proposed `model.rs`) enables independent testing.

- **`projection.rs:43` — `BASELINE_CACHE_TTL_S` comment says "re-check after this many seconds to avoid staleness across DST shifts"**
  The TTL is 6 hours. A DST transition that happens to fall during the cached window will serve stale `local_start_of_day_ms` until the TTL expires, potentially for up to 6 hours. The comment acknowledges this; the TTL value is a business-level tradeoff, not a bug, but the interaction with the DST issue in `local_start_of_day_ms` above compounds the risk.

---

### src-tauri/src/usage/poller.rs

#### MEDIUM
- **`poller.rs:131` — `forced` keys are consumed before the provider/pane guard**
  ```rust
  let forced: Vec<AccountKey> = std::mem::take(&mut *inner.force_refresh.lock());
  ```
  Forced keys are drained at the start of `tick`, then the provider list is computed. If a forced key's provider has no visible panes, the key is added to `candidates` (line 156-159) but `run_probe_for` will still be called. The probe itself will spawn a hidden pane and probe it, which is correct, but the forced key for an invisible provider bypasses the visibility guard that ordinarily gates probing. This is probably intentional for manual refresh, but it is undocumented.

- **`poller.rs:378-380` — `accounts_for_panes` always returns `[None]`**
  Multi-account support is stubbed: the function always returns a single `None` account regardless of the panes passed. The stub comment is absent; future multi-account work will need to revisit every call site.

#### LOW
- **`poller.rs:204-213` — jitter source uses `subsec_nanos` of wall clock**
  Jitter is derived from `SystemTime::now().subsec_nanos()`. This is deterministic within the same millisecond and predictable across calls (the nanos value at any given second is not random). The comment says "cheap deterministic-ish jitter" which is accurate, but callers happening to invoke this at aligned nanosecond boundaries would get the same offset every time. A `rand` call (already a dependency for token generation) would give true jitter.

---

### src-tauri/src/usage/probe.rs

#### MEDIUM
- **`probe.rs:176-180` — extra `\r` write after `/usage` is fire-and-forget**
  The `spawn_blocking` result for the extra carriage return is discarded with `let _ = ... .await`. An error here (PTY closed, bad pty_id) is silently dropped even though it could cause the probe to time out waiting for output that never arrives.

- **`probe.rs:241-251` — `screen_has_tui_input` uses hardcoded UI strings**
  Detection of the Claude TUI input box relies on literal strings (`"│ >"`, `"❯"`, `"│ Try"`, `"ctrl+l"`, etc.). The architecture notes in `CLAUDE.md` explicitly say "Detection uses structural fingerprints, never literal strings — vendors change copy without notice." This function is a direct violation of that rule.

#### LOW
- **`probe.rs:204` — `let _ = pane_id;` dead binding**
  `pane_id` is passed to `probe_inner` as a parameter but never used inside the function (all operations use `pty_id`). The suppression with `let _ = pane_id` indicates the parameter is vestigial. It should either be used or removed from the signature.

---

### src-tauri/src/update/shutdown.rs

#### LOW
- **`shutdown.rs:184` — `session.child.lock()` uses `std::sync::Mutex` — poisoning risk**
  `session.child` uses a `std::sync::Mutex`. If the child lock is poisoned (a thread panicked while holding it), `is_alive` returns `true` (line 184: `Err(_) => return true`), causing `poll_until_exit` to keep waiting until the timeout. This is the conservative/safe choice for the shutdown use case.

---

### Cross-cutting

#### MEDIUM
- **`activity.rs:116-119`, `health/http.rs:57-62`, `health/tcp.rs:41-46`, `health/process.rs:96-101`, `health/script.rs:73-78`, `services/store/crud.rs:7-12` — six copies of `fn now_ms()`**
  An identical 5-line `SystemTime::now()...as_millis() as i64` function is duplicated across six files. A single `crate::util::now_ms()` would eliminate the duplication.

- **Terminology drift: "cycle" vs "period" vs "billing_period"**
  `projection.rs` uses `cycle_end_ms` and `weekly_resets_at` interchangeably for the billing cycle reset timestamp. `UsageSnapshotRow` calls this `weekly_resets_at`. The `Baseline` struct has no explicit "cycle" concept. The public `ProjectionResponse` fields use `end_of_cycle_pct`. Three names for the same concept across the module.

---

## Summary Table

| Severity | Count |
|----------|-------|
| CRITICAL | 1     |
| HIGH     | 5     |
| MEDIUM   | 14    |
| LOW      | 12    |
| **Total**| **32**|
