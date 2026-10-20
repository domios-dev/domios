# Platform crate boundaries

Inventory baseline: `7a2f073cab499104dbceeee1da5c8e62f77f57a3`. Counts include tests where inline; each row is a
source file containing an OS conditional, not a claim that the entire file is OS-specific.
The source scan includes `target_os`, `windows`, and `unix` attributes.

The first native ownership cuts are the recording input adapters: Windows Raw
Input (2,336 lines including tests) and macOS IOHID (1,771 lines including tests).
They own device identity, descriptor validation, event decoding, native resources,
and acknowledged teardown. `codeterm-input-windows` depends only on
`codeterm-types`; `codeterm-input-macos` uses the standard library and system
frameworks. Shared HID declarations live in `codeterm-types::local_recording`.
The plugin registry retains grants, reservations and recording state. The app
retains generation checks, admission and callback translation.

`codeterm-pty` and the excluded `pty-fork` already own PTY/ConPTY mechanics.
Next cuts: process snapshots/start-time identities from `service/process_tree`,
ConPTY host cleanup from `update/conpty_cleanup`, and keep-awake OS adapters.
Provider tree witnesses and app events stay with their service owners. Process
identity must include the start token so PID reuse cannot authorize termination.
No lock ordering or input delivery behavior changes in this extraction.

## Verification and timing

Run `./build.sh nextest input-platform` in the warm canonical checkout on macOS,
Windows and Linux (Windows: `.\build.ps1 nextest input-platform`). It compiles
both library targets in one invocation; non-Windows builds contain no Raw Input
tests, while portable IOHID model tests run everywhere. The independent
`rust-input-platform` release-test matrix uses this selector. The two packages
are excluded from the full workspace archives to prevent duplicate execution;
remaining app/native tests keep their existing archived coverage.

Compare the baseline's
`./build.sh nextest app 'test(/^service::local_recording::platform::/)'`
with the new `./build.sh nextest input-platform` on the same OS/toolchain/cache
state. Record compiler completion time, execution time, test counts, raw exits,
source SHAs and whether target artifacts were warm. A dependency reduction is
source evidence, not a measured speedup. Before/after timings remain pending
canonical execution; this document makes no numerical speed claim.

## Source inventory

| Lines | Source at baseline |
| ---: | --- |
| 1197 | `src-tauri/src/storage/verification.rs` |
| 1184 | `src-tauri/src/service/orchestrator/spawn.rs` |
| 1182 | `src-tauri/src/service/pending_delivery_payloads/user_sends.rs` |
| 1115 | `src-tauri/src/service/pty_io/send/mod.rs` |
| 1106 | `src-tauri/src/service/orchestrator/launch.rs` |
| 1094 | `src-tauri/src/socket/server/mod.rs` |
| 1055 | `src-tauri/src/service/mod.rs` |
| 1048 | `src-tauri/src/crash/lifecycle.rs` |
| 1020 | `src-tauri/src/lib.rs` |
| 980 | `src-tauri/src/storage/tests.rs` |
| 970 | `src-tauri/src/sentinel/probe/owner.rs` |
| 950 | `src-tauri/src/keepawake.rs` |
| 908 | `src-tauri/src/service/general_agent/instructions.rs` |
| 884 | `src-tauri/src/service/lane_removal.rs` |
| 819 | `src-tauri/src/commands/skills.rs` |
| 814 | `src-tauri/src/sentinel/command_tests.rs` |
| 760 | `src-tauri/src/sentinel/tests.rs` |
| 758 | `src-tauri/src/socket/server/tab.rs` |
| 750 | `src-tauri/src/update/apply.rs` |
| 731 | `src-tauri/src/update/conpty_cleanup.rs` |
| 715 | `src-tauri/src/service/pty_io/tests.rs` |
| 681 | `src-tauri/src/service/lane_removal/tests.rs` |
| 675 | `src-tauri/src/service/pending_delivery_payloads.rs` |
| 662 | `src-tauri/src/service/provider_update/command.rs` |
| 630 | `src-tauri/src/service/worker_lane/tests.rs` |
| 615 | `src-tauri/src/service/process_tree.rs` |
| 606 | `src-tauri/src/sentinel/probe/tests.rs` |
| 586 | `src-tauri/src/service/codex_rpc/lifecycle.rs` |
| 568 | `src-tauri/src/service/codex_rpc/process.rs` |
| 560 | `src-tauri/src/service/team/staffing/tests.rs` |
| 554 | `src-tauri/src/sentinel/command_native_tests.rs` |
| 548 | `src-tauri/src/service/local_recording/setup_tests.rs` |
| 540 | `src-tauri/src/sentinel/probe/native_tests.rs` |
| 530 | `src-tauri/src/sentinel/probe/group_retirement.rs` |
| 503 | `src-tauri/src/storage/migration_tests.rs` |
| 500 | `src-tauri/src/service/pending_delivery_payloads/user_sends/tests.rs` |
| 491 | `src-tauri/src/api/os_notify.rs` |
| 484 | `src-tauri/src/socket/server/plan_task_dispatch/tests/checkout_occupancy.rs` |
| 482 | `src-tauri/src/startup/cli_install.rs` |
| 470 | `src-tauri/src/startup/frontend_shell.rs` |
| 451 | `src-tauri/src/update/manifest.rs` |
| 451 | `src-tauri/src/liveness.rs` |
| 428 | `src-tauri/src/storage/migration_tests/capture.rs` |
| 422 | `src-tauri/src/service/codex_rpc/launch.rs` |
| 401 | `src-tauri/src/service/provider_update/process.rs` |
| 384 | `src-tauri/src/api/bridge_routes.rs` |
| 373 | `src-tauri/src/webview2_reap.rs` |
| 369 | `src-tauri/src/service/agent_pad/tests.rs` |
| 365 | `src-tauri/src/service/local_recording/platform/macos.rs` |
| 364 | `src-tauri/src/service/provider_update.rs` |
| 362 | `src-tauri/src/crash/shutdown.rs` |
| 361 | `src-tauri/src/service/provider_update/command/process_fixture.rs` |
| 339 | `src-tauri/src/update/mod.rs` |
| 332 | `src-tauri/src/sentinel/probe/capture.rs` |
| 320 | `src-tauri/src/service/provider_update/command/tests.rs` |
| 311 | `src-tauri/src/autostart.rs` |
| 309 | `src-tauri/src/sentinel/probe/cancellation/unix.rs` |
| 307 | `src-tauri/src/socket/server/plan_task_dispatch/tests/lane_persist.rs` |
| 307 | `src-tauri/src/service/local_recording/platform.rs` |
| 301 | `src-tauri/src/sentinel/probe/process.rs` |
| 278 | `src-tauri/src/runaway_sweeper.rs` |
| 276 | `src-tauri/src/agent/run_watcher/cron_completion/tests.rs` |
| 270 | `src-tauri/src/startup/user_path.rs` |
| 267 | `src-tauri/src/api/local_preview_routes/tests/fixture.rs` |
| 263 | `src-tauri/src/keepawake/tests.rs` |
| 244 | `src-tauri/src/crash/image_identity.rs` |
| 239 | `src-tauri/src/service/agent_runtime/process.rs` |
| 238 | `src-tauri/src/sentinel/probe.rs` |
| 236 | `src-tauri/src/crash/exit.rs` |
| 227 | `src-tauri/src/startup/cli_shim.rs` |
| 225 | `src-tauri/src/service/supporter/tests.rs` |
| 224 | `src-tauri/src/shell.rs` |
| 216 | `src-tauri/src/crash/mod.rs` |
| 215 | `src-tauri/src/service/account.rs` |
| 215 | `src-tauri/src/sentinel/probe/cancellation.rs` |
| 214 | `src-tauri/src/commands/html_preview/file.rs` |
| 210 | `src-tauri/src/sentinel/probe/environment.rs` |
| 206 | `src-tauri/src/service/pty_io/inject.rs` |
| 196 | `src-tauri/src/clipboard_files.rs` |
| 189 | `src-tauri/src/service/supporter/journal.rs` |
| 187 | `src-tauri/src/service/tokens.rs` |
| 184 | `src-tauri/src/service/codex_rpc/unix_lease.rs` |
| 184 | `src-tauri/src/service/agent_runtime/lease.rs` |
| 181 | `src-tauri/src/service/health/process.rs` |
| 169 | `src-tauri/src/api/fs_routes.rs` |
| 162 | `src-tauri/src/crash/native.rs` |
| 154 | `src-tauri/src/service/capability_probe/process.rs` |
| 146 | `src-tauri/src/sentinel/probe/test_readiness.rs` |
| 143 | `src-tauri/src/commands/clipboard.rs` |
| 131 | `src-tauri/src/service/provider_update/storage.rs` |
| 128 | `src-tauri/src/service/health/script.rs` |
| 118 | `src-tauri/src/startup/window.rs` |
| 118 | `src-tauri/src/commands/system_info.rs` |
| 117 | `src-tauri/src/service/provider_update/process_tests.rs` |
| 107 | `src-tauri/src/process_util.rs` |
| 98 | `src-tauri/src/webview_watchdog.rs` |
| 95 | `src-tauri/src/service/tab_chat/file_events.rs` |
| 92 | `src-tauri/src/commands/system.rs` |
| 92 | `src-tauri/src/commands/html_preview/file/tests.rs` |
| 86 | `src-tauri/src/service/provider_signals_probe.rs` |
| 77 | `src-tauri/src/service/process_tree/argv0.rs` |
| 71 | `src-tauri/src/fdlimit.rs` |
| 69 | `src-tauri/src/service/agent_runtime/mod.rs` |
| 66 | `src-tauri/src/service/pty_io/mod.rs` |
| 39 | `src-tauri/src/service/supporter/storage.rs` |
| 38 | `src-tauri/src/service/local_recording/receiver_status.rs` |
| 35 | `src-tauri/src/update/install_holders.rs` |
| 34 | `src-tauri/src/service/codex_rpc/mod.rs` |
| 34 | `src-tauri/src/commands/file_actions.rs` |
| 31 | `src-tauri/src/startup/restart_stdio.rs` |
| 30 | `src-tauri/src/startup/conpty_host.rs` |
