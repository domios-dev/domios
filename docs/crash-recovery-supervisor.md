# Crash Recovery Supervisor

## Ownership

The daemon records startup, health, shutdown, and crash classification state, but it never restarts itself after a crash. Restart ownership belongs to `codeterm system supervise`, a CLI process started by OS login/install wrappers:

- Windows: `codeterm system enable-autostart` installs the `CodeTerm-Autostart` scheduled task. The task starts the CLI supervisor, not the app directly.
- macOS: the LaunchAgent starts the CLI supervisor with `system supervise --binary <app>`. `KeepAlive=false` prevents duplicate app instances and leaves restart authority in the supervisor loop.
- Linux: the user systemd unit starts the CLI supervisor with `system supervise --binary <app>`. `Restart=no` prevents systemd from overriding clean-exit and loop-break decisions.

The supervisor adopts a live process with a fresh matching PID/epoch heartbeat instead of launching a duplicate. It writes `pending_crash.json` before backoff or restart, so a supervisor interruption cannot erase the crash or reset the loop counter. The next daemon binds that external observation to its new epoch and promotes it to `last_crash.json`. Explicit user exit and signals stop supervision. Update and requested-restart handoffs remain clean: the supervisor follows the replacement epoch when it appears, but never launches the replacement itself.

## Markers

Durable marker location is `<data_dir>/state/recovery/`:

- `current.json`: current process identity, epoch, PID, build hash, start time, and last healthy timestamp.
- `last_clean.json`: clean shutdown/update/restart/user-exit marker with epoch and reason.
- `last_crash.json`: latest abnormal classification with old/new epoch, last healthy timestamp, backoff, and work policy.
- `pending_crash.json`: external supervisor classification written before restart, including fast-fail launches that never reached daemon startup.
- `supervisor.disabled`: explicit durable autostart-disable intent; an already-running supervisor checks it before any recovery launch.
- `crash_history.json`: bounded rolling history used for loop protection.

Crash report artifacts remain in `<data_dir>/crashes/`. Recovery markers are small and retained until overwritten; crash history is pruned to the loop-protection window.

## Work Recovery Policy

Startup restores session snapshots and task bindings before any worker spawn. Existing task execution bindings are authoritative. Safe idempotent work may resume only when the provider/session contract proves the same attempt can continue without duplicate side effects. Ambiguous active execution is marked `needs-review/reassign`; it is never marked safe by absence of evidence.

Report routing follows restored owner/planner bindings. Duplicate worker and report checks are required before any supervisor-assisted recovery can dispatch more work.

The supervisor never resumes workers. It only restarts the app process; restored planner/task state remains owned by existing task_23fywk startup invariants.

## UI Placement

Recovery status is a durable full-width top-level banner surfaced from `get_recovery_state`, with event refresh on new crash-report discovery. It has a stable `recovery-status-banner` test selector and no dismiss action. The banner is visible only when the crash record is bound to the current recovery epoch, so a later clean launch supersedes it without deleting audit history.

## Platform Oracle

- Windows: the scheduled task starts immediately when enabled and at logon. Force-kill the supervised app, assert `pending_crash.json` precedes the replacement epoch, then verify the task remains one supervisor with one app child. `/End` plus `/Delete` owns disable.
- macOS: the loaded LaunchAgent runs one CLI supervisor with `KeepAlive=false`. Repeat the kill, clean-exit, replacement-handoff, and loop-threshold oracle; validate the generated plist with `plutil`.
- Linux: the enabled user unit runs one CLI supervisor with `Restart=no`. Repeat the same oracle and validate the generated unit with `systemd-analyze --user verify`; quoted `ExecStart` paths must survive spaces.
