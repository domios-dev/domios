---
name: utility-shell-reports
description: Route dev readiness and QA summaries from utility shells back to the calling agent tab.
---

# Utility shell reports

Use this skill when a dev launcher or QA run needs to report to an agent tab,
especially when a named dev instance or a command pipeline can change where a
bare `codeterm` call or exit status would go.

Open build shells with `--purpose build` and foreground servers with `--purpose dev`. Purpose is retained when restarting the shell. The sidebar command state reports shell marks only; use the readiness and QA receipts below to establish the operation outcome.

Set `--timeout 10m` for tests, QA and builds; leave servers unset. Reset or extend the same shell with `codeterm tab timeout --tab <ID> --timeout 20m`; `--timeout none` disables it. An idle expiry closes quietly; running or unknown activity produces one owner status and terminates with the shell. Normal command completion adds no message.

Shell restart preserves the original deadline. Reset it before another build if that build needs a fresh lifetime. `codeterm tab inspect --tab <ID>` shows `utility_deadline.started_ms` and `deadline_ms` in epoch milliseconds; their difference is the requested lifetime. Timeout notices distinguish shell lifetime from the latest command elapsed time.

With `QA_NOTIFY_TAB`, QA streams each step to its log and sends one early
failure receipt on the first Cargo compile error, nextest/Vitest failure,
Node `not ok`, or non-zero exit (including gateway refusal 75 and cap 124).
The receipt names the label, step, first failure and log path. The command
continues, and the final summary still arrives. Dev `--notify-tab` uses the
same stream detector across setup and compilation; compile failure or exit
before READY reports immediately. No sleep or polling drives failure detection.

## Commands

For a health-proven dev startup that exits when ready:

```bash
./build.sh dev --wait --notify-tab <agent-tab-id>
./build.sh dev-prebuilt --wait --notify-tab <agent-tab-id>
```

If a named instance is already running with stale health, stop it before
restarting: `CODETERM_INSTANCE=<n> ./build.sh kill`.

For a visible foreground dev process, use the same flag without `--wait`:

```bash
./build.sh dev --notify-tab <agent-tab-id>
./build.sh dev-prebuilt --notify-tab <agent-tab-id>
```

The foreground stream reader treats each output line as a cue to validate `/api/health`
against the expected build and instance. It reports ready only after that proof,
or reports failure at process exit or the single 600-second deadline.

Run QA commands through a fresh report directory:

```bash
scripts/qa-report.sh smoke 'true' 'node --test scripts/tests/dev-wait.test.mjs'
QA_NOTIFY_TAB=<agent-tab-id> scripts/qa-report.sh smoke 'true' 'false | tail -1'
```

## Launcher recipes (copy-paste)

`check`, `check-queue`, `check-tests`, `test-one`, `nextest`, `nextest-archive`, `test`, `verify`, `release-preflight`, `build` and `install-local` take
`--notify-tab <tab>` (or `BUILD_NOTIFY_TAB`, or `-NotifyTab` through `build.ps1`): one `OK` receipt on success, or one
early-failure receipt naming the step, first error line and log path (gateway refusal 75 included), sent
through the captured parent context. `install-local` runs rustfmt, the file-size gate and the clean-checkout check
before compilation; Cargo formatting uses the same gateway entry as other Cargo verbs.

The Windows utility shell is PowerShell, where `&&` is invalid: set the directory with `--cwd` and never chain.
`<checkout>` is this checkout's absolute path; `<tab>` is the calling tab (bash `$CODETERM_TAB_ID`,
PowerShell `$env:CODETERM_TAB_ID`, inside double quotes).

| Operation | PowerShell caller and shell | bash caller and shell |
|---|---|---|
| install-local | `codeterm tab new --team <team-id> --purpose build --timeout 10m --cwd "<checkout>" --cmd ".uild.ps1 install-local -NotifyTab $env:CODETERM_TAB_ID"` | `codeterm tab new --team <team-id> --purpose build --timeout 10m --cwd "<checkout>" --cmd "./build.sh install-local --notify-tab $CODETERM_TAB_ID"` |
| check | `... --cmd ".uild.ps1 check -NotifyTab $env:CODETERM_TAB_ID"` | `... --cmd "./build.sh check --notify-tab $CODETERM_TAB_ID"` |
| check-queue | `... --purpose qa --cmd ".\build.ps1 check-queue <sha> -NotifyTab $env:CODETERM_TAB_ID"` | `... --purpose qa --cmd "./build.sh check-queue <sha> --notify-tab $CODETERM_TAB_ID"` |
| test-one | `... --cmd ".uild.ps1 test-one app NAME -NotifyTab $env:CODETERM_TAB_ID"` | `... --cmd "./build.sh test-one app NAME --notify-tab $CODETERM_TAB_ID"` |
| release-preflight | `... --cmd ".uild.ps1 release-preflight -NotifyTab $env:CODETERM_TAB_ID"` | `... --cmd "./build.sh release-preflight --notify-tab $CODETERM_TAB_ID"` |
| QA batch (`check` first) | `... --purpose qa --cmd ".uild.ps1 qa-batch smoke -NotifyTab $env:CODETERM_TAB_ID 'node --test scripts/tests/dev-wait.test.mjs'"` | `... --purpose qa --cmd "./build.sh qa-batch smoke --notify-tab $CODETERM_TAB_ID 'node --test scripts/tests/dev-wait.test.mjs'"` |
| dev (ready receipt) | `... --purpose dev --cmd ".uild.ps1 dev -NotifyTab $env:CODETERM_TAB_ID"` | `... --purpose dev --cmd "./build.sh dev --notify-tab $CODETERM_TAB_ID"` |

`...` is `codeterm tab new --team <team-id> --purpose build --timeout 10m --cwd "<checkout>"`. `qa-batch <label> '<cmd>'...`
always runs `./build.sh check` as step one, then your commands, and sends one QA summary.

## Recovering a lane whose recorded base is wrong

Run in the canonical checkout. `keep` refuses a dirty worktree, parks the tip on `keep/<lane>`, removes the worktree and
the lane branch; re-dispatch the lane from the correct base; then `merge` real-merges the kept ref and deletes it once
it is an ancestor.

```
bash ./build.sh lane-recover keep <lane-branch> <worktree-path>
bash ./build.sh lane-recover merge <lane-branch>
```

`bash ./build.sh ...` runs unchanged on bash and PowerShell with Git Bash on PATH; `.uild.ps1 lane-recover keep <lane-branch> <worktree-path>` is the PowerShell-only form.

From PowerShell, use:

```powershell
.\build.ps1 dev --wait -NotifyTab <agent-tab-id>
```

Create one Team-bound QA tab for repeated reports, then restart it for each run:

```bash
codeterm tab new --team <team-id> --purpose qa --timeout 10m --title QA
codeterm tab timeout --tab <qa-tab-id> --timeout 10m
codeterm tab restart --tab <qa-tab-id> --cmd "QA_NOTIFY_TAB=<agent-tab-id> scripts/qa-report.sh smoke 'true' 'false | tail -1'"
```

The send uses `codeterm send --tab <agent-tab-id> --from-file <body-file>`.
Every report body is written to a file before it is sent.

## Shared Git config lock before Bash

If a PowerShell utility shell stops at `git config core.hooksPath` with a config
lock error, the launcher still contains the old redundant setup write. The
PowerShell wrapper now forwards directly; build.sh owns best-effort hook setup.
Use the updated wrapper and retain the original failed shell receipt. No Cargo
or gateway job ran when the old wrapper stopped before forwarding.

## Parent environment trap

`build.sh` captures the parent `codeterm` path and its connection and request
identity values before `setup_env` and before named-instance exports. The dev
CLI is later put first on `PATH`, and `CODETERM_INSTANCE` / `CODETERM_API_PORT`
can override the caller's socket selection and then select the dev daemon, even
when set by a command prefix before `build.sh` starts. The notification helper
clears every `CODETERM_*` variable, restores the captured parent URL, socket,
token, tab, and request identity, and leaves `CODETERM_INSTANCE` and
`CODETERM_API_PORT` unset before invoking the captured CLI path.

Bench helpers use a dev-pinned `dc` wrapper, and named-instance exports retarget
bare `codeterm` calls to dev. Capture the parent CLI and connection before those
instance exports are applied.

`qa-report.sh` captures that same parent context before running commands. A
command that exports named-instance values cannot redirect the report.

## Pipe exit status trap

Use `bash -o pipefail -c` for each QA command. Without `pipefail`,
`false | tail -1` returns the status of `tail` and can look successful. The
foreground dev launcher reads `PIPESTATUS` immediately after teeing output so
the real `pnpm tauri dev` result is preserved.

## Stale result trap

Each QA run writes a full log, per-command logs, and a summary under a new
`<label>-<timestamp>-<pid>` directory. A failed build can leave older committed
result files behind; choosing a result with `ls -t` can mistake stale output for
the current run. Read the path printed for the fresh run.

## Filtered Cargo targets

A focused CLI test can execute in one target while Cargo prints `running 0 tests`
for unrelated integration targets. Count actual passed/failed tests across Cargo
result lines before declaring the command empty. Keep genuinely empty or
ignored-only runs and independent empty test runners as failures. Verify the
counter with `node --test --test-name-pattern=qa_report scripts/tests/utility-shell-reports.test.mjs`;
retain the original receipt instead of rerunning successful Rust tests.

## Event waits

The foreground reader consumes pipe lines as events and validates health on each
line. It uses one bounded deadline and cleans up its stream reader when ready,
failed, or stopped. Use this event wait instead of adding sleep or polling
loops.

<!-- revision: 10 -->

DEV app stdout/stderr use .NET Framework synchronous pipes on Windows and regular
files on Unix, through `scripts/dev-app-process.mjs`. The reader forwards log chunks
into the report stream, preserving READY health validation without exposing the
Rust app to overlapped pipeline handles. Both foreground and `--wait` share it.

## PowerShell capture turns Cargo progress into an exception

Do not wrap `build.ps1` with PowerShell `*>` or `2>&1` to capture build output.
Its `ErrorActionPreference=Stop` can turn normal Cargo stderr progress into a
terminating `NativeCommandError`, leaving no compile verdict or exit record.
Run the command through `scripts/qa-report.sh` in the Team utility shell instead;
it merges streams inside Bash and preserves the raw exit in the run summary.
