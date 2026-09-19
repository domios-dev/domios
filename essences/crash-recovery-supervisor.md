# Crash Recovery Supervisor

## Ownership

The daemon records startup, health, shutdown, and crash classification state, but it never restarts itself after a crash. Restart ownership belongs to `codeterm system supervise`, a CLI process started by OS login/install wrappers. It is a **hidden** clap subcommand — deliberately absent from `codeterm system --help` and from `generated/cli/cli.md`, because the only supported way to install it is `codeterm system enable-autostart`. Never invoke it by hand; the wrappers are:

- Windows: `codeterm system enable-autostart` installs the `CodeTerm-Autostart` scheduled task. The task starts the CLI supervisor, not the app directly.
- macOS: the LaunchAgent starts the CLI supervisor with `system supervise --binary <app>`. `KeepAlive=false` prevents duplicate app instances and leaves restart authority in the supervisor loop.
- Linux: the user systemd unit starts the CLI supervisor with `system supervise --binary <app>`. `Restart=no` prevents systemd from overriding clean-exit and loop-break decisions.

The supervisor adopts a live process with a fresh matching PID/epoch heartbeat instead of launching a duplicate. It writes `pending_crash.json` before backoff or restart, so a supervisor interruption cannot erase the crash or reset the loop counter. The next daemon binds that external observation to its new epoch and promotes it to `last_crash.json`. Explicit user exit and signals stop supervision. Update and requested-restart handoffs remain clean: the supervisor follows the replacement epoch when it appears, but never launches the replacement itself.

## Markers

Durable marker location is `<data_dir>/state/recovery/`:

- `current.json`: current process identity, epoch, PID, build hash, start time, and last healthy timestamp.
- `last_clean.json`: clean shutdown/update/restart/user-exit marker with epoch and reason.
- `last_crash.json`: latest abnormal classification with old/new epoch, last healthy timestamp, backoff, work policy, binding evidence, and the `healed_at` stamp once the epoch proves healthy.
- `pending_crash.json`: external supervisor classification written before restart, including fast-fail launches that never reached daemon startup.
- `supervisor.disabled`: explicit durable autostart-disable intent; an already-running supervisor checks it before any recovery launch.
- `crash_history.json`: bounded rolling history used for loop protection.

Crash report artifacts remain in `<data_dir>/crashes/`. Recovery markers are small and retained until overwritten; crash history is pruned to the loop-protection window.

## Classification Evidence

Classification describes the dead epoch, never the restart. The daemon reads only what the previous epoch recorded about itself: an epoch whose healthy lifetime (`last_healthy_at - started_at`) never reached 30s is `fast_fail`; any other unclean epoch is `abnormal_exit`. Elapsed time between the death and the next launch is downtime, not evidence — a machine left off overnight restarts into a plain `abnormal_exit`, not a kill report.

`power_loss_or_kill` is reserved for an external observer that actually watched the process die. Only `pending_crash.json`, written by `codeterm system supervise`, carries it; the daemon promotes that observation verbatim and never infers it.

## Work Recovery Policy

Startup restores session snapshots and task bindings before any worker spawn. Existing task execution bindings are authoritative. Safe idempotent work may resume only when the provider/session contract proves the same attempt can continue without duplicate side effects.

Ambiguity is a function of binding state, never of elapsed time. After the session snapshot is restored, `note_restored_bindings` records the exact Tab and provider-session identity of every role-bearing binding the dead epoch persisted and every binding that came back. The numeric `bound_at_shutdown` and `restored` fields remain a summary; health and ambiguity compare identities, so an unrelated replacement Tab cannot satisfy a missing binding. Only the exact-identity shortfall is ambiguous — `work_policy.ambiguous_execution` is `{ missing_bindings, disposition: "needs-review/reassign" }` when bindings were lost, and `null` when every one returned. A fifteen-minute restart with a full restore is therefore healthy, not ambiguous. Absence of evidence never marks work safe, and it never manufactures an accusation either.

Report routing follows restored owner/planner bindings. Duplicate worker and report checks are required before any supervisor-assisted recovery can dispatch more work.

The supervisor never resumes workers. It only restarts the app process; restored planner/task state remains owned by existing task_23fywk startup invariants.

## UI Placement

Recovery status is a toast surfaced from `get_recovery_state`, with event refresh on new crash-report discovery and on durable recovery-state changes. It exposes the accessible status name `Crash recovery status` and a dismiss action.

Presentation is driven by `crash_presentation`, derived from persisted state and never from a clock. It is `active` only while the crash is bound to the current epoch and that epoch has not yet been healed.

The warning's lifetime ends at proven health, not at user dismissal. Restored lazy agents are explicitly seeded as idle after snapshot restore; that readiness boundary and later live session/agent-state transitions compare the exact restored binding identities. A match durably stamps `healed_at` and emits `recovery-state-changed`, so an already-mounted toast disappears without a refresh. Fresh launches also check on the real `BootStage::AgentReady` transition. None of these transitions uses elapsed time as readiness. Because the heal is persisted rather than recomputed, the refresh path re-reads a healed record and stays quiet; a false warning is unrepresentable once health is proven. A later epoch supersedes the record for the same reason. `last_crash.json` and `crash_history.json` survive the transition untouched — the audit stays readable while the accusation retires.

## Platform Oracle

- Windows: the scheduled task starts immediately when enabled and at logon. Force-kill the supervised app, assert `pending_crash.json` precedes the replacement epoch, then verify the task remains one supervisor with one app child. `/End` plus `/Delete` owns disable.
- macOS: the loaded LaunchAgent runs one CLI supervisor with `KeepAlive=false`. Repeat the kill, clean-exit, replacement-handoff, and loop-threshold oracle; validate the generated plist with `plutil`.
- Linux: the enabled user unit runs one CLI supervisor with `Restart=no`. Repeat the same oracle and validate the generated unit with `systemd-analyze --user verify`; quoted `ExecStart` paths must survive spaces.

