# Behaviour benches

All behavioural verification (system prompts, session restore, recall
quality, agent routing) lives under `evals/<name>/`. Cross-platform
helpers in `evals/_lib/lib.sh` keep individual benches short and
portable across mac, linux, and Windows-bash.

## Existing sets

| Folder | What it tests | Runner |
|--------|---------------|--------|
| `cli-agent/` | real-daemon LLM benchmark: drives claude/codex/opencode in the dev console through every CLI command; model-judged (see its `SKILL.md`). Replaces the old headless `-p` general-agent / mem-protocol prompt evals. | `bash cli-agent/run-suite.sh` + model judge |
| `mem-recall/` | mem search engine quality (Recall@K / MRR / latency) | `cargo run --bin mem-bench` |
| `dreaming/` | REM-phase prompt — Stage A.1 hub decomposition (RED ZONE rule), Stage A.2 island bridging (5 cases) | `prompt-evaluator` skill |
| `session-restore/` | kill+restart preserves workspaces / tabs / agent_type / titles | `bash session-restore/run.sh` |

### mem-recall baseline protocol

`evals/mem-recall/` keeps a committed `baseline.json` (headline metrics only).

- Normal update: `./evals/mem-recall/run.sh` → update `baseline.json` from the latest
  run output if the change in recall / latency is intentional.
- Regression check: `./evals/mem-recall/run.sh --check` compares latest metrics against
  `evals/mem-recall/baseline.json` and fails non-zero on drift beyond tolerance.
- Tolerance policy in `evals/mem-recall/run.sh` is documented in its `--help` output.

Reviewer expectation for behavioral changes: include the updated baseline file in the same change that alters
mem-recall behavior, and keep both diff and intent explicit in the PR.

### session-restore baseline protocol

`evals/session-restore/` keeps a committed `baseline.json` for the six `WS-*`
restore scenarios. Dynamic tab ids and agent session ids are excluded.

- Normal update: `bash evals/session-restore/run.sh` → inspect
  `target-check/evals/session-restore-latest-summary.json` and update
  `evals/session-restore/baseline.json` only when the restore scenario shape
  intentionally changes.
- Regression check: `bash evals/session-restore/run.sh --check` runs the full
  dev kill/restart drill and compares the latest summary against the baseline.
- Tolerance policy in `evals/session-restore/run.sh --help`: `WS-*` scenario
  metadata is exact; the protected `General` workspace tab/tab counts may vary
  by ±1 around restart.

## Live dashboard

For long `prompt-evaluator` runs (anything with `--thorough` or A/B
mode) add `--http-port 7779` to the runner — it spawns an in-process
HTTP server with a single-page dashboard at `http://localhost:7779`:
progress bar, per-eval status pills, expand-on-click rows showing the
prompt, every run's response, and per-expectation matcher verdicts.
Server stays up after the run for browsing — Ctrl-C to stop.

## Why a shared lib

A full bench drill is dozens of `curl`/`codeterm`/`python` calls.
Inlining all of that in every bench (a) wastes tool-call budget when
the orchestrator runs the bench from a Claude session, and (b) hides
OS-specific commands (`taskkill` vs `kill -9`, named pipes vs Unix
sockets) inside individual benches that then drift across platforms.

`evals/_lib/lib.sh` collects every cross-cutting helper:

- `dev_resolve` (alias `dev_env`) — resolves the dev connection (port, token, socket path) into lib-private state; exports nothing
- `dev_run <cmd...>` — runs one child process with the dev connection composed into *its* environment
- `dev_auth_header` / `dev_data_dir` — the resolved values, for shell code that used to read an exported variable
- `dev_pid` / `dev_alive` — find the dev process via `lsof` (Unix) or `netstat` (Win)
- `dev_kill_hard` — `kill -9` (Unix) or `taskkill /F` (Win) → simulates Windows reboot / OOM
- `dev_wait_ready` — polls `/api/health` until dev is up
- `dev_start_bg` — starts `./build.sh dev` in the background
- `snapshot <label>` — saves `ws.json` + `tabs.json` + `last.json` to `target-check/evals/<label>/`
- `diff_snapshots <pre> <post>` — PASS/FAIL table over workspace + tab metadata
- `cleanup_test_workspaces [prefix]` — closes leftover `WS-*` workspaces from prior runs
- `assert_atomic_save_clean` — guards against corrupt `last.json` or leftover `.tmp`
- `new_workspace`, `new_shell_tab`, `new_claude_tab` — one-line scenario builders

OS detection is via `$OSTYPE` (works in git bash on Windows). All
commands are POSIX-portable; Windows-only branches are isolated to
`is_windows` guards.

## Writing a new bench

```bash
#!/usr/bin/env bash
set -u
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"
dev_resolve || exit 1
dev_alive || { echo "start dev first"; exit 1; }

# 1. set up the scenario
WS=$(new_workspace WS-myfeature C:/temp/myfeature)
P=$(new_claude_tab $WS C:/temp/myfeature my-tab "initial task")

# 2. snap, kill, restart
sleep 35
PRE=$(snapshot myfeature-pre)
dev_kill_hard
assert_atomic_save_clean || exit 1
dev_start_bg
dev_wait_ready
POST=$(snapshot myfeature-post)

# 3. verify
diff_snapshots "$PRE" "$POST"
```

Three notes for new benches:

1. **Always source the lib via `git rev-parse --show-toplevel`** so the
   bench works regardless of where it's invoked from.
2. **Always go through `dc` / `dev_get` / `dev_run`.** A bare `codeterm` falls
   through to prod (port 7685) — that's how Andrey lost workspaces during a
   previous test. Sourcing the lib deliberately leaves your shell's own
   `CODETERM_*` alone, so the wrappers are what target dev, not the environment.
3. **Snapshot labels go under `target-check/evals/`** — gitignored,
   per-machine artifacts. Pre/post must use distinct labels.

## Running from a Claude session

Each helper is one tool call with clean output. A full restore drill
is six tool calls instead of thirty:

```bash
bash evals/session-restore/run.sh
```

Output ends with `PASS metadata diff` or `FAIL N drift(s)` plus a
manual-step reminder for the UI-mount verification.
