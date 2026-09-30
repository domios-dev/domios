---
name: utility-shell-reports
description: Route dev readiness and QA summaries from utility shells back to the calling agent tab.
---

# Utility shell reports

Use this skill when a dev launcher or QA run needs to report to an agent tab,
especially when a named dev instance or a command pipeline can change where a
bare `codeterm` call or exit status would go.

## Commands

For a health-proven dev startup that exits when ready:

```bash
./build.sh dev --wait --notify-tab <agent-tab-id>
./build.sh dev-prebuilt --wait --notify-tab <agent-tab-id>
```

For a visible foreground dev process, use the same flag without `--wait`:

```bash
./build.sh dev --notify-tab <agent-tab-id>
./build.sh dev-prebuilt --notify-tab <agent-tab-id>
```

The foreground waiter treats each log line as a cue to validate `/api/health`
against the expected build and instance. It reports ready only after that proof,
or reports failure at process exit or the single 600-second deadline.

Run QA commands through a fresh report directory:

```bash
scripts/qa-report.sh smoke 'true' 'node --test scripts/tests/dev-wait.test.mjs'
QA_NOTIFY_TAB=<agent-tab-id> scripts/qa-report.sh smoke 'true' 'false | tail -1'
```

From PowerShell, use:

```powershell
.\build.ps1 dev --wait -NotifyTab <agent-tab-id>
```

Create one Team-bound QA tab for repeated reports, then restart it for each run:

```bash
codeterm tab new --team <team-id> --title QA
codeterm tab restart --tab <qa-tab-id> --cmd "QA_NOTIFY_TAB=<agent-tab-id> scripts/qa-report.sh smoke 'true' 'false | tail -1'"
```

The send uses `codeterm send --tab <agent-tab-id> --from-file <body-file>`.
Every report body is written to a file before it is sent.

## Parent environment trap

`build.sh` captures the parent `codeterm` path and its connection and request
identity values before `setup_env` and before named-instance exports. The dev
CLI is later put first on `PATH`, and `CODETERM_INSTANCE` / `CODETERM_API_PORT`
then select the dev daemon. A bare `codeterm` at that point can send a report to
dev. The notification helper clears every `CODETERM_*` variable and restores
only the captured parent values before invoking the captured CLI path.

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

## Event waits

The foreground waiter consumes log lines as events and validates health on each
line. It uses one bounded deadline and cleans up its stream reader when ready,
failed, or stopped. Use this event wait instead of adding sleep or polling
loops.

<!-- revision: 1 -->
