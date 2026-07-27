# Internal QA gates (build machines)

Repo-local gate runner for release/integration verification on warm checkouts.
No daemon, socket, or mesh — orchestrators invoke `scripts/qa-gate.sh` directly.

## Quick start

```bash
# List gates + default timeouts
scripts/qa-gate.sh list

# Lane-friendly compile/format gate on current HEAD
scripts/qa-gate.sh run fmt check

# Integration branch gate (mirrors full pre-push hook)
scripts/qa-gate.sh run prepush-full --timeout 2400

# Verify a specific commit on this machine (fetch + checkout + sha pin)
scripts/qa-gate.sh run rust-unit ts-unit --ref feat/canvas-updates --expect-sha a2523df6 --fetch

# macOS builder native lane
scripts/qa-gate.sh run macos-native

# JSON for plan stride / automation
scripts/qa-gate.sh run check --json

# Smoke: passing gate exits 0 + prints QA_RUN (prod-tab env safe)
scripts/qa-gate-smoke.sh
```

## Machine output contract

First line of each gate result is parseable:

```
QA_GATE gate=fmt status=pass duration_secs=3 sha=a2523df6 branch=feat/canvas-updates host=codeterm-builder
```

Serial run summary:

```
QA_RUN status=pass passed=2 failed=0 duration_secs=120 sha=... branch=... host=...
```

Statuses: `pass`, `fail`, `timeout`, `skip` (platform/dev unavailable), `blocked` (dirty tree / bad sha).

Paste the KV lines into a plan stride milestone; prose logs land under `$TMPDIR/codeterm-qa-gate-<pid>/`.

## Gate catalog

| Gate | Typical machine | Notes |
|------|-----------------|-------|
| `shell-guards` | any | Fast mux/async/kill-safety scripts |
| `fmt` | any | `cargo fmt --check` |
| `check` | any | `cargo check` both crates |
| `clippy` | integration Mac/ROG | Full clippy gate |
| `rust-unit` / `cli-unit` | any | Capped at 90s via `test-capped.sh` |
| `ts-unit` / `plugin` | any | pnpm layers |
| `test-all` | dev-up machine | `build.sh test:all`; skips dev layers if 7686 down |
| `file-sizes` / `comment-length` | any | Discipline scripts `--fail` |
| `macos-native` | Mac builder | `test-macos-native.sh` |
| `linux-docker` | Mac w/ Docker | `test-linux-docker.sh` |
| `e2e-pytest` / `playwright-*` | dev-up machine | Requires healthy dev on 7686 |
| `prepush-light` | lane worktrees | Hook parity (fmt + guards) |
| `prepush-full` | integration branches | Hook parity (check + fmt + clippy) |

## Recommended release stride (manual orchestration)

Run **serially** on each builder; do not fan out per-worker compiles:

1. **Mac (`codeterm-builder`)**: `prepush-full` → `macos-native` → optional `playwright-golden` (dev up)
2. **ROG Windows**: `prepush-full` → platform build workflow (see `release-windows.yml`)
3. **ROG WSL Linux**: `prepush-full` → `linux-docker` or `release-linux.yml`

`scripts/test-matrix.sh` remains the parallel macOS+Linux lane runner; `qa-gate.sh` is the
**serial, named, stride-friendly** wrapper for orchestrator paste-back.

## Options

- `--ref` / `--fetch` / `--expect-sha` — pin checkout before gates
- `--allow-dirty` — skip clean-tree guard (local iteration only)
- `--timeout SECS` — override per-gate default
- `--skip-dev-check` — run dev gates even when 7686 is down (will likely fail)
- `--json` — JSON lines instead of KV

## Prod-tab safety

When launched from a prod CodeTerm orchestrator tab (`CODETERM_API_PORT=7685`), the runner
sanitizes inherited env before each gate: pins API port to **7686**, resets socket to dev,
clears tab identity. Explicit non-prod ports are kept. Opt out with `QA_GATE_ENV_LOCKED=1`;
named dev instance via `QA_GATE_INSTANCE=<name>`; custom socket via `QA_GATE_SOCKET_PATH`.

<!-- revision: 2 -->
