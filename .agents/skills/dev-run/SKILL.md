---
name: dev-run
description: Start, restart, kill, and verify the CodeTerm dev instance from inside this repo. Use whenever the user says "start dev", "restart dev", "dev up", "kill dev", "rebuild dev", asks to open/reload the app after code changes, reports the dev window is broken/not launching, or you need to verify that the running binary matches the source after an edit to src-tauri/ or src/. Covers build.sh commands, port 7686 health checks, log locations, clean-config resets, and platform-specific quirks.
---

# Dev Run — CodeTerm dev instance

Run, restart, and diagnose the CodeTerm dev instance inside this repo. Dev is isolated from production: different config dir, different port, different data.

**Never touch prod.** Prod listens on 7685, dev on 7686. If you ever kill a process, verify by port first.

This skill owns only development instances. Local production installation is
`.agents/skills/build/SKILL.md#local-production-build-and-self-install`; public publishing is
the `github-release` skill. Do not use `dev` output as an installer artifact.

## The one command

```bash
./build.sh dev
```

That's it. Hot-reloads Vite on every TSX edit, rebuilds Rust on `src-tauri/` edits, opens the Tauri window. Auto-builds CLI into the active `~/.codeterm-dev*/bin/` and routes dev PTYs to that binary.

Run it in the **background** so you can keep working:

```bash
# Foreground (blocks):  ./build.sh dev
# Background (use run_in_background=true via the Bash tool)
```

### Other `build.sh` verbs

| verb | what |
|---|---|
| `./build.sh dev` | start dev (port 7686, hot-reload) |
| `./build.sh kill` | stop dev only — prod on 7685 stays alive |
| `./build.sh check` | `cargo check` (~2s incremental) — use this before debugging to confirm code still compiles |
| `./build.sh test` | Rust unit tests |
| `./build.sh cli` | rebuild CLI only — no dev restart needed if you only edited `cli/` |
| `./build.sh clean` | wipe build cache, next build from scratch |
| `./build.sh webdriver` | dev with WebDriver enabled for UI tests |
| `./build.sh e2e` | run E2E suite against a running dev |

## Verifying the running binary matches your code

`build.sh kill` guarantees the old dev is dead before `build.sh dev` starts the new one — so any `/api/health` response after restart is from the fresh build.

When running from inside a production CodeTerm tab, the shell may inherit
`CODETERM_API_PORT=7685`. `build.sh dev`, `build.sh kill`, and e2e commands
guard against that and switch the canonical dev instance back to 7686; do not
override that guard unless you are intentionally running a named non-prod
instance on another port.

```bash
curl -s http://127.0.0.1:7686/api/health | jq '.status'
# → "ok"
```

If you need to identify the build, `build` carries the short git commit hash:

```bash
curl -s http://127.0.0.1:7686/api/health | jq '.build'
# → "31c581a"
```

The hash only changes when you commit. During iterative development it stays the same across restarts (and that's fine — the kill-then-start sequence is your "did the restart take" signal).

## Polling for readiness (after restart)

**Always poll, never `ScheduleWakeup` a fixed delay** — Rust re-link is unpredictable (5–60s depending on how much changed), and polling returns in milliseconds when it's ready. Fixed `sleep 90` or a 90s wakeup wastes time on a 15s rebuild and misses a 120s one.

Easiest: `./build.sh dev --wait` does this for you (kills, builds, polls /api/health, exits when ready or times out at 300s).

Manual polling:

```bash
./build.sh kill && ./build.sh dev &   # or run_in_background=true via the Bash tool
for i in $(seq 1 120); do
  if curl -fs --max-time 2 http://127.0.0.1:7686/api/health >/dev/null 2>&1; then
    echo "ready after ${i}s"
    break
  fi
  sleep 1
done
```

Runs up to 2 minutes at 1s intervals. Because `build.sh kill` ran first, the only thing that can answer is the new build.

**What NOT to do:**
- Don't `ScheduleWakeup` a blind delay for a rebuild — you don't know how long it'll take.
- Don't use `Monitor` on the background build task expecting to catch "ready" in the output. The Rust compile log is noisy and you'll get pinged on every `Compiling foo v0.x` line. Polling health is lower-overhead.
- Don't poll faster than 1s — `curl` on Windows has ~200ms overhead per call already, anything tighter just burns CPU for nothing.

### If the build takes > 2 min

Rare, but happens with `./build.sh clean && ./build.sh dev` or after a big refactor that invalidates the incremental cache. Bump the Bash tool timeout to 300000ms (5 min) and the loop counter to 300, OR poll in chunks by re-running the same loop.

## Logs

- **macOS/Linux:** `~/.codeterm-dev/logs/codeterm.log`
- **Windows:**    `%USERPROFILE%\.codeterm-dev\logs\codeterm.log`

Tail live:

```bash
tail -f ~/.codeterm-dev/logs/codeterm.log
```

The build.sh bg task's own output stream also contains the Rust compile log and Vite dev server output — check it when something goes wrong during startup, not after.

## Dev data location

### Explicit named-dev tailnet access

Keep `api.host` on loopback. A named development instance may opt into an
additional listener with `api.mesh_tailnet_host` set to its locally assigned
Tailscale IPv4 address. Use the named instance and port with the dev wrapper
for `dc config set api.mesh_tailnet_host '"<tailnet-ip>"'`; never a bare CLI
inheriting production routing. Confirm identity before changing that instance.
Restart only that named instance through the documented launcher. Verify both
loopback health and tailnet health, the build identity, and
`dc config get api_listener.ready_addresses` before `dc mesh broadcast`.
Removing the opt-in uses JSON `null` and another scoped restart.

A failed secondary bind starts neither listener; no wildcard or LAN fallback
is allowed. A later secondary listener exit keeps local HTTP serving but clears
remote readiness and stops broadcast. Recovery is an explicit scoped restart,
not a retry loop. Preserve a failed trial's state and distinguish a new trial.

Troubleshooting: `mesh_listener_not_remotely_ready` means no health-proven remote
listener can serve the announcement. A configured host is not live evidence.
Do not widen the default bind or repeatedly connect to a stale discovered
address; verify the named instance's live listener set first.

- `~/.codeterm-dev/` — config, mem DB, sessions, logs, mesh identity
- `~/.codeterm-dev/config.toml` — themes, keybindings, api section
- `~/.codeterm-dev/mem/` — mem SQLite + FTS + vectors
- `~/.codeterm-dev/sessions/` — persisted workspaces/tabs

**Important:** the dev CLI config is separate from prod. Dev PTYs receive the
active data-directory CLI first on `PATH`; production shells continue using
`~/.codeterm/bin/`. From outside a dev PTY, use the dev environment wrapper:

```bash
source evals/_lib/lib.sh
dc <subcommand>
```

The wrapper pins the dev socket, token, and port and refuses to run while dev is
down. Never use a bare production `codeterm` command as a dev bench shortcut.

## "Start from a clean dev" / засранный конфиг

When dev state is corrupted (mem DB weirdness, stale workspaces, broken mesh identity, etc.) and you want a fresh boot:

```bash
./build.sh kill
mv ~/.codeterm-dev ~/.codeterm-dev.bak-$(date +%Y%m%d-%H%M%S)
./build.sh dev
```

Rename, don't `rm -rf` — the backup stays around in case the user's in-progress state was in there. Only delete after they confirm it's safe.

## Auto-restart policy

**Always restart dev yourself after editing `src-tauri/` code** — don't ask. Rust changes need a Rust rebuild. TSX / TS changes under `src/` are hot-reloaded by Vite, no restart needed.

Changes to prompt markdown files (`src-tauri/prompts/*.md`, `.md` under `src-tauri/src/` included via `include_str!`) DO require a Rust rebuild because cargo hashes them at compile time.

## Workflow: I just edited X, now what?

| You edited… | Action |
|---|---|
| `src/**/*.tsx` / `src/**/*.ts` | nothing — Vite HMR picks it up |
| `src/index.css` or Tailwind | nothing — HMR |
| `src-tauri/src/**/*.rs` | `./build.sh kill && ./build.sh dev` in background, poll health |
| `src-tauri/prompts/**/*.md` | same as above — included via `include_str!` |
| `cli/src/**/*.rs` | `./build.sh cli` — no dev restart needed |
| `src-tauri/Cargo.toml` deps | `./build.sh clean && ./build.sh dev` |
| `package.json` deps | `bun install` (or npm), then dev restart |

## Process hygiene — NEVER kill prod

Before killing anything, check the port. Prod listens on 7685, dev on 7686.

```bash
# Dev PIDs (safe to kill):
lsof -nP -iTCP:7686 -sTCP:LISTEN -t
# Prod PIDs (NEVER kill):
lsof -nP -iTCP:7685 -sTCP:LISTEN -t
```

`./build.sh kill` already scopes to dev. Prefer it over manual `kill -9`.

---

## Troubleshooting log (grow this as things break)

> When you hit a dev-startup issue on any platform, add a dated entry below with: symptom, root cause, fix. Keep entries short.

If the failure is caused by the dev-run procedure itself, fix the procedure as
well as the immediate environment before declaring recovery complete.

### macOS

- **`build.sh dev` hangs during link, then SIGKILL** — usually not enough RAM (linker eats a lot). Close other heavy apps, retry. If it repeats, `./build.sh clean && ./build.sh dev`.
- **Health endpoint returns empty after `kill && dev`** — Rust is still linking. Poll the health endpoint for up to 40s before concluding the restart failed.
- **`caffeinate` stays running after dev crashes** — if a session used `codeterm keepawake start`, the caffeinate child is owned by the dev process and should die with it. If it doesn't: `pkill -x caffeinate` (safe — targets only the literal binary name).
- **Port 7686 already in use but no dev window** — stale process from a previous crash. `./build.sh kill` then start. If still occupied, `lsof -nP -iTCP:7686 -sTCP:LISTEN -t | xargs kill` and retry.
- **2026-09-30: first named `dev --wait` exits 1 silently after "codeterm-embed bundled"** — `record_dev_target` ran `cd` into `<instance>/target/debug`, which a fresh named target does not have yet (only `release/`), and `set -e` aborted without a message. `record_dev_target` now creates the directory; on an older checkout, `mkdir -p ~/.codeterm-dev-<name>/target/debug` and rerun.
- **`~/.codeterm-dev/mem/` corrupted / mem commands 500** — back it up and restart: `mv ~/.codeterm-dev ~/.codeterm-dev.bak-$(date +%s) && ./build.sh dev`.

### Windows

- **2026-07-25: process starts but 7686 never binds** — a temporary config guard
  in the `ServiceContext` literal was reacquired by a later field. Snapshot config
  once before constructing the context; do not erase dev state to mask the deadlock.
- **2026-09-28: next dev start fails with `os error 32`** — an orphaned
  `src-tauri/target/debug/conpty/x64/OpenConsole.exe` still has the build
  resource open after its daemon exits. A named interactive dev can build under
  this checkout's shared `src-tauri/target`, while named `dev --wait` builds
  under that instance's `target`; kill-time mode detection cannot distinguish
  where an already running instance built. Dev records its actual debug target
  in the instance data directory, and `build.sh kill` uses that record to find
  orphaned hosts. It reaps hosts only when their parent is gone or is the named
  instance's daemon, leaving another live instance's hosts alone on a shared
  target. Run the matching kill command, then start dev again; the daemon also
  closes hosts for PTYs it owns during shutdown.

### Linux

_(No entries yet — add as they're discovered.)_

---

## Rule: how this skill gets loaded

Claude Code should auto-load this skill whenever the user mentions:

- "start dev", "restart dev", "dev up", "kill dev", "rebuild"
- "the app isn't loading", "dev window broke", "can't connect to 7686"
- anything that implies running or diagnosing the local CodeTerm dev instance

Don't preemptively run `./build.sh dev` on every edit — only when the user asks or when you have a Rust/prompt change that definitely needs a live binary to verify.

<!-- revision: 1 -->
