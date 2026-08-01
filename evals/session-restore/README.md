# Session restore — manual + scripted bench

Verifies that workspace/tab state survives daemon restart, including
the trickier cases (silent claude tab, claude tab with user input,
custom-renamed tab, plain shell tab). Runs against a live `dev`
daemon on port 7686.

## Why this exists

Andrey lost four workspaces (NoPolicePlease, sibzoloto,
playnite-categorizer, almaz-admin) after a Windows reboot. Two root
causes:

1. `SessionStore::save` used `fs::write` (truncate+write, not atomic).
   A kill mid-write left `last.json` truncated; next boot's
   `serde_json::from_str` failed; the caller silently fell into the
   `_ =>` branch and booted with an empty workspace list.
2. `from_session_snapshot` dropped `agent_type` whenever
   `claude_session_id` wasn't saved, so a freshly-spawned "silent"
   claude tab (the user never typed into it before the kill) came
   back as a bare powershell on restart.

Fixes (PR #54):
- Atomic write via `last.json.tmp` + `fs::rename`.
- Backup-fallback chain in `load_last`: if `last.json` is missing or
  fails to parse, walk `backups/` from newest to oldest and return
  the first valid snapshot.
- `from_session_snapshot` now always preserves `agent_type`. The
  spawn-on-mount path (`commands::pty::spawn_pty`) decides whether to
  `--resume` (session_id present + valid file) or fresh-launch the
  agent. Gated on a new in-memory `restored_from_snapshot` flag so
  the fresh-launch does NOT race the orchestrator's own launch on a
  freshly spawned tab.
- `send_text` retries `resolve_pty_id` for up to 15s so scripts can
  chain `agent spawn → send` without hitting a "no PTY" race.

## Layers tested

| Layer | Tested by |
|-------|-----------|
| `SessionStore::save` atomic + `load_last` fallback | `cargo test --lib session::store` (6 unit tests) |
| `from_session_snapshot` agent_type preservation | `cargo test --lib multiplexer::state::tests::test_session_snapshot_*` |
| End-to-end restore on dev (kill → restart → state preserved) | `bash evals/session-restore/run.sh` (this directory) |
| Silent claude tab re-launches as claude on UI mount | **manual** — open dev window, click the restored tab, watch claude UI render |
| Claude --resume of saved session_id | **manual** — same as above; depends on the JSONL session file still existing on disk |

## Running the bench

Prereqs:
- dev daemon running on port 7686 (`./build.sh dev`)
- dev API token + socket path (the script reads them from
  `~/.codeterm-dev/config.toml` + the well-known pipe name)

```bash
bash evals/session-restore/run.sh
bash evals/session-restore/run.sh --check
```

The script:
1. Cleans any stale `WS-*` workspaces left from a previous run.
2. Creates 6 workspaces with mixed scenarios (empty, silent shell,
   shell-with-text, silent claude, claude-with-`--task`, claude
   custom-renamed).
3. Snapshots `pre/` state (workspaces, tabs, last.json).
4. `taskkill /F` the dev daemon (Windows shutdown simulator).
5. `./build.sh dev` to restart.
6. Snapshots `post/` state.
7. Seeds a newer worker-looking Claude JSONL in the General Agent cwd,
   wipes the General tab sid from `last.json`, and asserts restart still
   resumes the GA-owned sid.
8. Diffs and prints a PASS/FAIL table.
9. In `--check` mode, compares the latest metadata summary against
   `baseline.json` and exits non-zero on drift outside the documented
   tolerance.

Outputs go to `target-check/evals/` (gitignored).

## Test plan layout

| # | Workspace | Tab sequence | What restore must preserve |
|---|-----------|---------------|----------------------------|
| 1 | `WS-empty` | none | workspace id, name, cwd |
| 2 | `WS-shell-silent` | `tab new` | bare shell tab stays bare on UI mount |
| 3 | `WS-shell-with-text` | `tab new` + `tab exec` | same as above; scrollback NOT preserved (PTY restarts) |
| 4 | `WS-claude-silent` | `agent spawn claude` (no input) | **claude relaunches** on UI mount, not powershell |
| 5 | `WS-claude-with-text` | `agent spawn claude --task '...'` | claude launches and `--resume` picks up the conversation IF JSONL still on disk; fresh launch otherwise |
| 6 | `WS-claude-renamed` | `agent spawn claude` + `tab rename "MY-CUSTOM"` | custom title preserved + claude relaunches |

## Adding scenarios

Extend `run.sh` with another `create_ws + spawn` block, then add a
matching row to the verification python at the bottom. Keep each
scenario small and named — the diff output is grepped by name.

## When this fires in CI

Not yet — the bench needs a live dev daemon, taskkill, and human
verification of the UI-mount step. CI runs the unit tests
(`cargo test --lib session::store` and the multiplexer roundtrip
tests) which cover the file-format invariants. The end-to-end
behaviour is checked manually before merging anything that touches
`SessionStore`, `from_session_snapshot`, `commands::pty`, or
`startup::states`.

<!-- revision: 3 -->
