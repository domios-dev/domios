---
name: dev-run
description: Start, restart, kill, and verify the CodeTerm dev instance from inside this repo. Use whenever the user says "start dev", "restart dev", "dev up", "kill dev", "rebuild dev", asks to open/reload the app after code changes, reports the dev window is broken/not launching, or you need to verify that the running binary matches the source after an edit to src-tauri/ or src/. Covers build.sh commands, port 7686 health checks, log locations, clean-config resets, and platform-specific quirks.
version: 0.3.0
---

# Dev Run — CodeTerm dev instance

Run, restart, and diagnose the CodeTerm dev instance inside this repo. Dev is isolated from production: different config dir, different port, different data.

**Never touch prod.** Prod listens on 7685, dev on 7686. If you ever kill a process, verify by port first.

## The one command

```bash
./build.sh dev
```

That's it. Hot-reloads Vite on every TSX edit, rebuilds Rust on `src-tauri/` edits, opens the Tauri window. Auto-builds CLI into `~/.codeterm/bin/` on every start.

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

`build.sh dev` bumps `BUILD_NUMBER` on every launch. After a restart, confirm the instance you're hitting is the new one:

```bash
curl -s http://127.0.0.1:7686/api/health | jq '{build, build_number}'
# → {"build":"<commit-hash>","build_number":"123"}
```

If `build` (git hash) or `build_number` is the old value, the restart didn't take — the old binary is still running. Kill it explicitly and try again.

## Polling for readiness (after restart)

**Always poll, never `ScheduleWakeup` a fixed delay** — Rust re-link is unpredictable (5–60s depending on how much changed), and polling returns in milliseconds when it's ready. Fixed `sleep 90` or a 90s wakeup wastes time on a 15s rebuild and misses a 120s one.

Copy this exact loop after starting dev in the background:

```bash
for i in $(seq 1 120); do
  h=$(curl -s http://127.0.0.1:7686/api/health 2>/dev/null)
  if [ -n "$h" ]; then echo "$h"; break; fi
  sleep 1
done
```

Runs up to 2 minutes at 1s intervals in the foreground. As soon as the health endpoint responds, it prints the JSON (which includes `build` hash and `build_number`) and exits. The Bash tool's default 2-minute timeout matches the 120 iterations so you don't need to bump it.

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

- `~/.codeterm-dev/` — config, mem DB, sessions, logs, mesh identity
- `~/.codeterm-dev/config.toml` — themes, keybindings, api section
- `~/.codeterm-dev/mem/` — mem SQLite + FTS + vectors
- `~/.codeterm-dev/sessions/` — persisted workspaces/tabs/panes

**Important:** the dev CLI config is separate from prod. The `codeterm` binary in `~/.codeterm/bin/` reads prod's `~/.codeterm/` identity by default — it does NOT auto-target dev. To hit dev from CLI, you either need to run from inside a dev PTY (env vars are set there) or add the dev port explicitly:

```bash
CODETERM_API_PORT=7686 codeterm <subcommand>
```

Even then, auth will fail unless the dev mesh identity matches — prod-signed tokens are rejected by dev. Cleanest path: just use curl with manual token from `~/.codeterm-dev/config.toml` `[api] token`, or run commands from inside a dev pane.

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

### macOS

- **`build.sh dev` hangs during link, then SIGKILL** — usually not enough RAM (linker eats a lot). Close other heavy apps, retry. If it repeats, `./build.sh clean && ./build.sh dev`.
- **Health endpoint returns empty after `kill && dev`** — Rust is still linking. Poll the health endpoint for up to 40s before concluding the restart failed.
- **`caffeinate` stays running after dev crashes** — if a session used `codeterm keepawake start`, the caffeinate child is owned by the dev process and should die with it. If it doesn't: `pkill -x caffeinate` (safe — targets only the literal binary name).
- **Port 7686 already in use but no dev window** — stale process from a previous crash. `./build.sh kill` then start. If still occupied, `lsof -nP -iTCP:7686 -sTCP:LISTEN -t | xargs kill` and retry.
- **`~/.codeterm-dev/mem/` corrupted / mem commands 500** — back it up and restart: `mv ~/.codeterm-dev ~/.codeterm-dev.bak-$(date +%s) && ./build.sh dev`.

### Windows

_(No entries yet — add as they're discovered.)_

### Linux

_(No entries yet — add as they're discovered.)_

---

## Rule: how this skill gets loaded

Claude Code should auto-load this skill whenever the user mentions:

- "start dev", "restart dev", "dev up", "kill dev", "rebuild"
- "the app isn't loading", "dev window broke", "can't connect to 7686"
- anything that implies running or diagnosing the local CodeTerm dev instance

Don't preemptively run `./build.sh dev` on every edit — only when the user asks or when you have a Rust/prompt change that definitely needs a live binary to verify.
