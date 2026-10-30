---
name: utility-shell-reports
description: Route dev readiness and QA summaries from utility shells back to the calling agent tab.
---

# Utility shell reports

Use this skill when a dev launcher or QA run needs to report to an agent tab,
especially when a named dev instance or a command pipeline can change where a
bare `domios` call or exit status would go.

Every script run in a utility shell streams its output into the terminal while it runs (owner requirement): pipe each step through `tee <log>` and take the exit status from `${PIPESTATUS[0]}`, print a header line per step, and never redirect a long step only into a file. The owner watches progress in the tab; the log file is a copy.

Open build shells with `--purpose build` and foreground servers with `--purpose dev`. Purpose is retained when restarting the shell. The sidebar command state reports shell marks only; use the readiness and QA receipts below to establish the operation outcome.

Set `--timeout 60m` for tests, QA and builds; leave dev servers unset. Each command-start mark starts a fresh deadline, and a finish mark cancels it. Change the bound for current and future commands with `domios tab timeout --tab <ID> --timeout 60m`; `none` disables both. The current command keeps its original start. On expiry the owned process tree stops through shell restart, the tab stays open, and one owner status names the command, command elapsed and deadline. Idle shells have no deadline; existing owner/Team cleanup still applies.

Shell restart retains the bound. Pass `domios tab restart --tab <ID> --timeout 60m` to override it, or `--timeout none` to disable it. `domios tab inspect --tab <ID>` shows `utility_deadline.duration_ms` and nullable `started_ms`, `deadline_ms` (epoch milliseconds). Only shell command marks establish command start.

With `QA_NOTIFY_TAB`, QA streams each step to its log and sends one early
failure receipt on the first Cargo compile error, nextest/Vitest failure,
Node `not ok`, or non-zero exit (including gateway refusal 75 and cap 124).
The receipt names the label, step, first failure and log path. The command
continues, and the final summary still arrives. Every Domios `dev` and
`dev-prebuilt` startup uses the same stream detector across setup and
compilation; compile failure or exit before READY reports immediately. No
sleep or polling drives failure detection.

## Commands

For a health-proven dev startup that exits when ready:

```bash
./build.sh dev --wait
./build.sh dev-prebuilt --wait
```

Inside Domios, build.sh inspects the current shell tab and sends the ready or
failure receipt to its live parent/owner. `--notify-tab <tab>` (or
`BUILD_NOTIFY_TAB`) explicitly overrides that recipient. A remote shell uses
its persisted mesh return route. If there is no parent, build.sh raises a
`domios notify` from the shell's AgentPad (`verbose` when ready,
`failure` on startup errors).

If a named instance is already running with stale health, stop it before
restarting: `CODETERM_DEV_INSTANCE=<n> CODETERM_DEV_API_PORT=<port> ./build.sh kill`.

For a visible foreground dev process, omit `--wait`:

```bash
./build.sh dev
./build.sh dev-prebuilt
```

The foreground stream reader treats each output line as a cue to validate `/api/health`
against the expected build and instance. It reports ready only after that proof,
or reports failure at process exit or the single 600-second deadline.

Run QA commands through a fresh report directory:

Lane checks run directly as `build.ps1 lane-check <crate> --tests` in a Team
utility shell. Do not wrap them in `qa-batch`: that verb prepends a canonical
`check` before its supplied commands. If used accidentally in a lane, close
that utility shell and restart with the authorized direct `lane-check` verb.

```bash
scripts/qa-report.sh smoke 'true' 'node --test scripts/tests/dev-wait.test.mjs'
QA_NOTIFY_TAB=<agent-tab-id> scripts/qa-report.sh smoke 'true' 'false | tail -1'
```

## Launcher recipes (copy-paste)

`check`, `check-queue`, `check-tests`, `test-one`, `nextest`, `nextest-archive`, `test`, `verify`, `release-preflight`, `build` and `install-local` resolve the recipient from the current Domios tab by default. They send one short success receipt or one failure receipt with the step, first compiler error line and log path (gateway refusal 75 included). `--notify-tab <tab>` (or `BUILD_NOTIFY_TAB`, or `-NotifyTab` through `build.ps1`) overrides the recipient. `install-local` runs rustfmt, the file-size gate and the clean-checkout check before compilation; Cargo formatting uses the same gateway entry as other Cargo verbs.

The Windows utility shell is PowerShell, where `&&` is invalid: set the directory with `--cwd` and never chain.
`<checkout>` is this checkout's absolute path; `<tab>` is the calling tab (bash `$CODETERM_TAB_ID`,
PowerShell `$env:CODETERM_TAB_ID`, inside double quotes).

| Operation | PowerShell caller and shell | bash caller and shell |
|---|---|---|
| install-local | `domios tab new --team <team-id> --purpose build --timeout 10m --cwd "<checkout>" --cmd ".\\build.ps1 install-local"` | `domios tab new --team <team-id> --purpose build --timeout 10m --cwd "<checkout>" --cmd "./build.sh install-local"` |
| check | `... --cmd ".\\build.ps1 check"` | `... --cmd "./build.sh check"` |
| check-queue | `... --purpose qa --cmd ".\\build.ps1 check-queue <sha>"` | `... --purpose qa --cmd "./build.sh check-queue <sha>"` |
| test-one | `... --cmd ".\\build.ps1 test-one app NAME"` | `... --cmd "./build.sh test-one app NAME"` |
| release-preflight | `... --cmd ".\\build.ps1 release-preflight"` | `... --cmd "./build.sh release-preflight"` |
| QA batch | `... --purpose qa --cmd ".\\build.ps1 qa-batch smoke -NotifyTab $env:CODETERM_TAB_ID 'true' 'node --test scripts/tests/dev-wait.test.mjs'"` | `... --purpose qa --cmd "./build.sh qa-batch smoke --notify-tab $CODETERM_TAB_ID 'true' 'node --test scripts/tests/dev-wait.test.mjs'"` |
| dev | `... --purpose dev --cmd ".\\build.ps1 dev"` | `... --purpose dev --cmd "./build.sh dev"` |
| dev-prebuilt | `... --purpose dev --cmd ".\\build.ps1 dev-prebuilt"` | `... --purpose dev --cmd "./build.sh dev-prebuilt"` |

The `--notify-tab` values in the table are optional overrides; omit them to use
the shell's inspected parent/owner route. `...` is `domios tab new --team
<team-id> --purpose build --timeout 10m --cwd "<checkout>"`. `qa-batch <label>
'<cmd>'...` always runs `./build.sh check` as step one, then your commands,
and sends one QA summary.

## Recovering a lane whose recorded base is wrong

Run in the canonical checkout. `keep` refuses a dirty worktree, parks the tip on `keep/<lane>`, removes the worktree and
the lane branch; re-dispatch the lane from the correct base; then `merge` real-merges the kept ref and deletes it once
it is an ancestor.

```
bash ./build.sh lane-recover keep <lane-branch> <worktree-path>
bash ./build.sh lane-recover merge <lane-branch>
```

`bash ./build.sh ...` runs unchanged on bash and PowerShell with Git Bash on PATH; `./build.ps1 lane-recover keep <lane-branch> <worktree-path>` is the PowerShell-only form.

From PowerShell, use:

```powershell
./build.ps1 dev --wait
```

Create one Team-bound QA tab for repeated reports, then restart it for each run:

```bash
domios tab new --team <team-id> --purpose qa --timeout 10m --title QA
domios tab restart --tab <qa-tab-id> --cmd "QA_NOTIFY_TAB=<agent-tab-id> scripts/qa-report.sh smoke 'true' 'false | tail -1'"
```

The send uses `domios send --tab <agent-tab-id> --from-file <body-file>`.
Every report body is written to a file before it is sent.

## Shared Git config lock before Bash

If a PowerShell utility shell stops at `git config core.hooksPath` with a config
lock error, the launcher still contains the old redundant setup write. The
PowerShell wrapper now forwards directly; build.sh owns best-effort hook setup.
Use the updated wrapper and retain the original failed shell receipt. No Cargo
or gateway job ran when the old wrapper stopped before forwarding.

## Parent environment trap

`build.sh` captures the parent `domios` path and its connection and request
identity values before `setup_env` and named-instance exports. It inspects the
current shell tab through that captured CLI and resolves the live parent,
creator, or persisted mesh return route. `CODETERM_PARENT_TAB_ID` is only a
fallback when tab inspection is unavailable. Explicit `--notify-tab` wins.
The notification helper clears every `CODETERM_*` variable, restores the
captured parent URL, socket, token, tab and request identity, and leaves
`CODETERM_INSTANCE` and `CODETERM_API_PORT` unset before invoking the captured
CLI path.

Bench helpers use a dev-pinned `dc` wrapper, and named-instance exports retarget
bare `domios` calls to dev. Capture the parent CLI and connection before those
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

Notification fixture tests that expect a rustfmt failure must use `fmt-check`,
with a fake rustfmt tool. `install-local` refuses with exit 2 on Linux and
macOS before formatting, so it cannot prove cross-platform failure routing.

A focused CLI test can execute in one target while Cargo prints `running 0 tests`
for unrelated integration targets. Count actual passed/failed tests across Cargo
result lines before declaring the command empty. Keep genuinely empty or
ignored-only runs and independent empty test runners as failures. Verify the
counter with `node --test --test-name-pattern=qa_report scripts/tests/utility-shell-reports.test.mjs`;
retain the original receipt instead of rerunning successful Rust tests.

Zero-test detection reads libtest `test result:`, nextest `Summary [...] N tests run:`,
and Node `# pass N` / `# fail N` summaries. Inventory rows containing `0 tests`
are not runner results and must never turn a successful validation into a failure.

## Event waits

The foreground reader consumes pipe lines as events and validates health on each
line. It uses one bounded deadline and cleans up its stream reader when ready,
failed, or stopped. Use this event wait instead of adding sleep or polling
loops.

<!-- revision: 13 -->

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

## Generated shell recipe startup

Validate a generated Bash recipe with `bash -n` before submitting that exact file.
The launch wrapper must also report a nonzero startup exit: a syntax error can
prevent the recipe's own failure reporter from being defined. A header file or
idle shell mark does not prove that any verification step ran.
