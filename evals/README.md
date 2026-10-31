# Test families and benches

Start with the test family, not this folder name.

- **Curated owner journeys:** [QA catalogue](qa/README.md) and its [required preread](qa/REPEATABLE-QA.md). Built-in Browser scenarios reuse reviewed actions and oracles from the suspended Playwright archive; archive inventory is not current coverage.
- **Deterministic unit/contract/API tests:** [testing procedure](../.agents/skills/testing/SKILL.md), without real models. Offline helper tests under `evals/` belong here too.
- **Load/performance:** measure latency, throughput, growth and stability under a specified workload; retain environment and measurements.
- **Agent evals:** measure behavior/quality, operationally beside load benches with explicit model/data/version pins and time/token budgets. A single quality task need not be a load test.

[`tests/catalogue.json`](../tests/catalogue.json) indexes runners; [`qa/catalogue.json`](qa/catalogue.json) indexes curated cases and its existing matrix records results. A procedure never authorizes a restart, model spend, sign-in or external action. Bench fixture actions use [`_lib/lib.sh`](_lib/lib.sh) through `dc`, `dev_get`, `dev_post` or `dev_run`; Browser management uses its own host connection.

## Existing sets

| Folder | What it tests | Runner |
|--------|---------------|--------|
| `cli-agent/` | real-daemon LLM benchmark: drives claude/codex/opencode in the dev console through every CLI command; model-judged (see its `SKILL.md`). Replaces the old headless `-p` general-agent / mem-protocol prompt evals. | `bash cli-agent/run-suite.sh` + model judge |
| `mem-recall/` | mem search engine quality (Recall@K / MRR / latency) | `cargo run -p codeterm-devtools --features dev-tools --bin mem-bench` |
| `dreaming/` | REM-phase prompt — Stage A.1 hub decomposition (RED ZONE rule), Stage A.2 island bridging (5 cases) | `prompt-evaluator` skill |
| `browser-discovery/` | a fresh agent with a plain web task picks the built-in browser plugin, not a provider browser extension | `bash browser-discovery/run.sh [provider[:model]...]` |
| `session-restore/` | kill+restart preserves workspaces / tabs / agent_type / titles | `bash session-restore/run.sh` |
| `core-journeys/` | release blocker: per-provider spawn/send/reply/stop, Team start from the dialog with agent tabs selected, chat_backend dispatch/ACK/report/review, DEV restart restore | `bash core-journeys/run.sh all` |
| `neighbor-friend-receiving/` | Opt-in named QA peers: move one friend, preserve another, reset and cold-restart the nonreceiver; requires four provisioned peers and a private manifest | `bash neighbor-friend-receiving/run.sh --help` |

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
- `dc --as-tab <id> ...`, `dev_run --as-tab <id> ...`, `dev_post_as_tab <id> <path> [json]` — run with that DEV tab's own scoped launch credential, never the instance token plus an asserted tab ID
- `dev_auth_header` / `dev_data_dir` — the resolved values, for shell code that used to read an exported variable
- `dev_pid` / `dev_alive` — find the dev process via `lsof` (Unix) or `netstat` (Win)
- `dev_kill_hard` — `kill -9` (Unix) or `taskkill /F` (Win) → simulates Windows reboot / OOM
- `dev_wait_ready` — consumes the startup acknowledgement and delegates bounded health readiness to `scripts/server-readiness.mjs`
- `dev_start_bg` — starts `./build.sh dev` in the background
- `snapshot <label>` — saves `ws.json` + `tabs.json` to `target-check/evals/<label>/`
- `diff_snapshots <pre> <post>` — PASS/FAIL table over workspace + tab metadata
- `cleanup_test_workspaces [prefix]` — closes leftover `WS-*` workspaces from prior runs
- `assert_atomic_save_clean` — guards against corrupt `last.json` or leftover `.tmp`
- `new_workspace`, `new_shell_tab`, `new_claude_tab` — one-line scenario builders
- `new_provisioned_workspace` — the daemon creates the workspace directory, so agents started in it get directory trust

OS detection is via `$OSTYPE` (works in git bash on Windows). All
commands are POSIX-portable; Windows-only branches are isolated to
`is_windows` guards.

Tab-scoped probes must run inside the selected DEV tab's launch environment,
with its `CODETERM_API_TOKEN`, tab ID, API URL/port and socket path intact before
sourcing the library. The requested tab must match that environment; its session
ID and binding nonce are retained for the child. An owner shell or a production
tab cannot impersonate an arbitrary DEV tab: the helpers refuse with exit 7.
Use owner `dc tab exec --tab <shell-tab> '<probe command>'` to start a probe inside a DEV shell
tab; for an agent tab, have the agent execute the probe with its own tool.
`tab exec` intentionally refuses agent tabs. Never print or copy credentials
through terminal output, inspect metadata, or substitute an instance token.
Omitting `--as-tab` still runs setup/owner operations against verified DEV only.

The mocked helper regression check is `bash evals/_lib/test-source-purity.sh`;
it contacts no daemon and covers scoped credentials, foreign/missing credential
refusals, unchanged caller environment and production target isolation.

## Writing a new bench

Use the maintained [curated case procedure](qa/README.md), [core journeys](core-journeys/README.md) or [startup matrix](startup-matrix/README.md) appropriate to the behavior. Declare target/build/boot/session identity, owned resources, budget, final oracle and cleanup before action. Restart drills require their own grant. Wait on events, DOM state, protocol acknowledgements or exact process exit under a failure deadline; elapsed time never proves readiness.

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
