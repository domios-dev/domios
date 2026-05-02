# Self-update

`codeterm self-update --apply` is the script-orchestrated upgrade path.
Where `update --apply` (Tauri-plugin-updater) handles the binary swap
inside the running process, `self-update --apply` hands off to an
external orchestrator script that runs *after* the daemon dies, polls
`/api/health` to verify the new version came up, and restores the
previous binary from `.bak` if anything's off.

This is the path actually exercised on `BLACKSTAR` for v1.1.2; the code
just bakes the manual ritual into a single CLI command and adds proper
Telegram notifications around it.

## Flow

```
codeterm self-update --apply
        │
        ▼
1. Resolve latest tag (skip if up-to-date)
2. Download platform asset to data_dir/upgrade-staging/
3. Render orchestrator script + UpgradeContext
4. Schedule script with platform dispatcher (schtasks on Windows)
5. Send Telegram "starting" notification through the bridge
6. Graceful shutdown (mem WAL, sessions, /exit panes)
        │
        ▼ daemon exits
7. (script) wait, kill survivors, snapshot .bak,
   run installer /S, start new exe, poll /api/health,
   roll back from .bak on mismatch, write upgrade-result.json
        │
        ▼ new daemon boots
8. boot-pickup reads upgrade-result.json,
   sends Telegram "done / rolled back / rollback failed",
   deletes the file.
```

## Files written

All under `data_dir` (`~/.codeterm/` for prod, `~/.codeterm-dev/` for dev):

| Path                                  | Owner    | Purpose |
|---------------------------------------|----------|---------|
| `upgrade-staging/`                    | daemon   | Cleared at the start of every `--apply`. |
| `upgrade-staging/<asset.exe>`         | daemon   | Downloaded NSIS / DMG / AppImage. |
| `upgrade-staging/run.ps1`             | daemon   | Orchestrator script (regenerated each apply). |
| `upgrade-staging/context.json`        | daemon   | Snapshot of `UpgradeContext` for debugging. |
| `upgrade-<old>-to-<new>.log`          | script   | Step-by-step log written by the orchestrator. |
| `upgrade-result.json`                 | script   | Final outcome; deleted by the new daemon after notification. |

## JSON contract

`upgrade-result.json` is the contract between the orchestrator script
(writer) and the next-boot pickup (reader). The schema lives in
`src-tauri/src/update/result.rs` so both sides reference the same
struct.

```json
{
  "status": "success | rolled_back | rollback_failed",
  "old_version": "1.1.2",
  "new_version": "1.1.3",
  "observed_version": "1.1.3",
  "channel": "stable",
  "log_path": "C:\\Users\\u\\.codeterm\\upgrade-1.1.2-to-1.1.3.log",
  "started_at": "2026-05-02T10:00:00Z",
  "finished_at": "2026-05-02T10:00:42Z",
  "error": "",
  "notify_chat_id": 12345
}
```

The script only writes the file. The daemon (next boot) does the
Telegram round-trip — that way external scripts don't need to handle
auth or HTML escaping for the bot API. `notify_chat_id` is just
plumbed through.

## Rollback

Two routes:

1. **Automatic** — the script does it itself when `/api/health` shows
   a version other than the target. Status `rolled_back` ends up in
   `upgrade-result.json`.
2. **Manual** — `codeterm self-update --rollback` schedules a separate
   rollback script that copies `.bak → install`, restarts, and writes
   the result. Use this when the new build came up *cleanly* but you
   later decide it's broken.

## Platform support

| OS      | `--apply` | `--rollback` | Dispatcher |
|---------|-----------|--------------|------------|
| Windows | yes       | yes          | `schtasks` one-shot |
| macOS   | falls back to `update --apply`  | n/a | TODO: `launchctl` / `at` |
| Linux   | falls back to `update --apply`  | n/a | TODO: `at` / fork+setsid |

The trait + module layout is already in place
(`src-tauri/src/update/orchestrator/{windows,macos,linux}.rs`); macOS
and Linux just need their own dispatcher impls.

## Why not call Telegram from the script?

We tried. PowerShell's `Invoke-RestMethod` body encoding for the
Telegram bot API kept hitting 401s in practice (Bearer scheme +
multipart edge cases). The fix is "don't talk to Telegram from the
script at all" — the script writes a result file, the daemon (which
already has a battle-tested bridge) sends the message. This makes the
script smaller, removes a third-party dependency from a critical
recovery path, and keeps the Telegram token off disk in any extra
location.

## Smoke testing on dev

```bash
# 1. Start dev codeterm
./build.sh dev

# 2. From any pane (or via REST)
~/.codeterm-dev/bin/codeterm self-update --check
~/.codeterm-dev/bin/codeterm self-update --apply --channel rc

# 3. Watch
tail -f ~/.codeterm-dev/logs/codeterm.log
tail -f ~/.codeterm-dev/upgrade-*.log

# 4. After the new daemon boots, confirm cleanup
ls ~/.codeterm-dev/upgrade-result.json     # should be gone
```

The dev install lives at `~/.codeterm-dev/bin/codeterm.exe` — distinct
data dir, port 7686, never touches `~/.codeterm/` or production.

<!-- revision: 1 -->
