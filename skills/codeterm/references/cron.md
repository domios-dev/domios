# Periodic Tasks (Cron)

Schedule recurring or one-time tasks. **Never use built-in CronCreate/CronList tools** — CodeTerm has its own cron that spawns Claude agents on schedule and tracks results.

## Recurring jobs

5-field cron expression: `min hour dom mon dow`.

```bash
codeterm cron create "Heartbeat" --schedule "0 * * * *" --task "Say hello"
codeterm cron create "Build Check" --schedule "*/30 * * * *" --task "Run cargo check and report"
codeterm cron create "Daily Review" --schedule "0 9 * * *" --task "Review TODOs" --cwd /path/to/project
```

Common expressions:
- `*/30 * * * *` — every 30 minutes
- `0 9 * * *` — daily at 9am
- `0 9 * * 1` — Mondays at 9am
- `0 * * * *` — every hour

## Oneshot jobs

Fire once, then auto-delete. Schedule accepts relative (`"in 5m"`, `"in 1h30m"`, `"in 2d"`), ISO 8601, or unix ms.

```bash
codeterm cron create "Quick Backup" --schedule "in 30m" --task "run backup" --once
codeterm cron create "Deploy Tonight" --schedule "2026-04-05T02:00:00Z" --task "deploy to prod" --once
```

## Management

```bash
codeterm cron list [--workspace ID]       # list all
codeterm cron edit ID [--name N] [--schedule S] [--task T]   # modify in place
codeterm cron enable ID
codeterm cron disable ID
codeterm cron trigger ID                  # fire now
codeterm cron delete ID                   # cascade: kills running tabs + removes logs
codeterm cron history ID                  # run history
```

**Always use `cron edit` to modify** — deleting and recreating loses history and changes the ID.

## How it works

- The cron scheduler spawns a Claude agent on schedule with the task text
- The spawned agent MUST call `codeterm cron complete --run-id UUID --summary "result"` when done (this instruction is auto-appended to the task prompt)
- Completing a run closes the run's tab (run tabs are ephemeral)
- Logs: `~/.codeterm/crons/<job_id>/<run_id>.log` (SUMMARY + RAW OUTPUT sections)
- The app hides to system tray on close so crons keep running
- Successful spawns do not generate notifications — only errors do
- `cron create` lands in your own workspace (the one owning your tab), never the one selected in the UI; override with `--workspace ID`

The sidebar shows a cron tree with runs and next scheduled time. Click a finished run to open the log viewer.
