# Crash-report false positive on graceful shutdown

> From Andrey 2026-06-20: Cmd+Q (or an external kill, e.g. ./build.sh) is recorded as a
> CRASH → next launch shows the crash-report toast. Fix the false positive without
> losing real crash reporting. Branch `plugins-rethink`.

## Root cause (prime suspect)
The native handler (`crash/native.rs`) only catches genuine crashes (SIGSEGV/SIGABRT/SEH)
— it does NOT fire on Cmd+Q/SIGTERM. The false positive comes from the **liveness
watchdog** (`liveness.rs`): it probes the local REST API from outside the request stack
and writes a `crash-<ts>-liveness.txt` report when the API goes unhealthy past a
threshold. During a graceful quit the API stops responding inside the shutdown window →
the watchdog interprets that as an outage → writes a crash report → `check_previous_crashes`
(`crash/mod.rs`) finds it next launch → `crash_report_available` → CrashReportToast.
(A teardown panic during shutdown is a secondary possible source.)

## Fix
1. **Global `SHUTTING_DOWN: AtomicBool`** (in `crash` or a small `shutdown` module),
   readable everywhere. Set `true` at the start of every graceful-exit route:
   - Tauri `RunEvent::ExitRequested` / `Exit` (lib.rs:~866) — covers Cmd+Q + window close.
   - The updater's `graceful_shutdown` (update/shutdown.rs) — already a route.
2. **Unix signal handler** for SIGTERM + SIGINT (covers `./build.sh kill` / Ctrl-C):
   set `SHUTTING_DOWN=true`, run the graceful cleanup, exit cleanly. (SIGKILL can't be
   caught, but then the in-process watchdog dies too and writes nothing — fine.)
   Windows: the Tauri exit event already covers WM_CLOSE/quit.
3. **Liveness watchdog guard:** before `write_liveness_report*`, return early if
   `SHUTTING_DOWN` is set; also stop the probe loop once shutting down. A clean quit's
   API-down is expected, not a crash.
4. **(Belt-and-suspenders) clean-exit sentinel:** on graceful exit write
   `crashes/.clean_exit` with the exit timestamp; `check_previous_crashes` ignores any
   report with mtime ≤ that timestamp. Primary defense is the SHUTTING_DOWN guard; this
   catches any racey report written right at exit.
5. **Preserve real crashes:** SIGSEGV/SIGABRT/native + genuine panics during normal run
   still write + surface. Only graceful-shutdown-window reports are suppressed.

## Verify
- Cmd+Q the dev app → NO new `crash-*` file written → relaunch shows NO toast.
- `./build.sh kill` (SIGTERM) → same: no report, no toast next launch.
- A forced crash (the existing crash-test trigger / a real panic mid-run) → STILL writes
  + surfaces the toast (regression guard that we didn't break real reporting).
- Test: `handle_probe_action`/`write_liveness_report` skips when SHUTTING_DOWN; sentinel
  filtering in `check_previous_crashes`.

## Task
- **CR-FIX (Rust backend, Opus):** implement 1–5 above; wire SHUTTING_DOWN at the exit
  routes + Unix SIGTERM/SIGINT handler; guard the liveness watchdog; add the clean-exit
  sentinel + `check_previous_crashes` filter; unit tests for the watchdog guard + sentinel.
  Gates: cargo build + test --lib + clippy + fmt + check-blocking-in-async. ROG verifies
  the Cmd+Q/kill→no-toast behavior on Windows.

<!-- revision: 1 -->
