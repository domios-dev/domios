---
name: dev-run
description: Start, restart, kill, and verify the Domios dev instance from inside this repo. Use whenever the user says "start dev", "restart dev", "dev up", "kill dev", "rebuild dev", asks to open/reload the app after code changes, reports the dev window is broken/not launching, or you need to verify that the running binary matches the source after an edit to src-tauri/ or src/. Covers build.sh commands, port 7686 health checks, log locations, clean-config resets, and platform-specific quirks.
---

# Dev Run — Domios dev instance

Run, restart, and diagnose the Domios dev instance inside this repo. Dev is isolated from production: different config dir, different port, different data.

**Never touch prod.** Prod listens on 7685, dev on 7686. If you ever kill a process, verify by port first.

This skill owns only development instances. Local production installation is
`.agents/skills/build/SKILL.md#local-production-build-and-self-install`; public publishing is
the `github-release` skill. Do not use `dev` output as an installer artifact.

## The one command

```bash
./build.sh dev
```

Builds the CLI and app, then launches the window through `dev-prebuilt`.
Vite `build --watch` refreshes the frontend bundle; Rust edits need another `dev`.
All build logic lives in `build.sh`; `build.ps1` forwards to it.
All Cargo work, including named `dev --wait`, owns the shared build gateway and
initializes sccache through its common entry. The build writes the existing dev
provenance record, closes gateway logging and releases the job before invoking
`dev-prebuilt`, which validates that record and owns launch, READY and cleanup.
A running app holds no gateway job; `kill` stops it through the existing dev-only
process cleanup. Re-running `dev` acquires the gateway for its build again.

Run it in the **background** so you can keep working:

```bash
# Foreground (blocks):  ./build.sh dev
# Background (use run_in_background=true via the Bash tool)
```

Agent launchers for dev, build, check, QA batch and install-local (PowerShell and bash lines, `--notify-tab` receipts): [utility-shell-reports](../utility-shell-reports/SKILL.md#launcher-recipes-copy-paste). Manual live drills: `scripts/hand-tests/README.md`; PowerShell isolation check against a warm dev: `scripts/check-dev-instance-isolation.ps1`.

### Other `build.sh` verbs

| verb | what |
|---|---|
| `./build.sh dev` | start dev (port 7686, hot-reload) |
| `./build.sh kill` | stop dev and reap its Codex RPC sidecar groups by token-file directory; prod stays alive |
| `./build.sh check` | `cargo check` (~2s incremental) — use this before debugging to confirm code still compiles |
| `./build.sh test` | Rust unit tests |
| `./build.sh cli` | rebuild CLI only — no dev restart needed if you only edited `cli/` |
| `./build.sh clean` | wipe build cache, next build from scratch |
| `./build.sh webdriver` | dev with WebDriver enabled for UI tests |
| `./build.sh e2e` | run E2E suite against a running dev |

`kill` also reaps orphaned Codex RPC launchers whose `--ws-token-file` is
inside this instance's `codex-rpc` directory. Unix cleanup requires an owned
group leader; Windows cleanup terminates the identified launcher's tree.
A launcher in an unidentified Unix group fails cleanup rather than risking
another instance. Inspect the scoped count with:

```bash
node scripts/reap-dev-sidecars.mjs "$HOME/.codeterm-dev" --count
```

For dev readiness reports routed to the calling agent tab, see [`utility-shell-reports`](../utility-shell-reports/SKILL.md).

`--notify-tab` reports the first compile error during startup immediately,
or reports an exit before READY. The shared stream reader covers setup and
compilation; foreground readiness also consumes the live pipe. See the
reporting skill above for receipts and parent routing.

## Verifying the running binary matches your code

`build.sh kill` guarantees the old dev is dead before `build.sh dev` starts the new one — so any `/api/health` response after restart is from the fresh build.

When running from inside a production Domios tab, the shell may inherit
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

Keep `api.host` on loopback. Any development instance (default or named) may opt into an
additional listener with `api.mesh_tailnet_host` set to its locally assigned
Tailscale IPv4 address. Use the named instance and port with the dev wrapper
for `dc config set api.mesh_tailnet_host '"<tailnet-ip>"'`; never a bare CLI
inheriting production routing. Confirm identity before changing that instance.
Restart only that named instance through the documented launcher. Verify both
loopback health and tailnet health, the build identity, and
`dc config get api_listener.ready_addresses` before `dc mesh broadcast`.
Removing the opt-in uses JSON `null` and another scoped restart.

To join two DEV consoles on different machines over the mesh (pairing,
managed-by tabs, web UI peer picker, leak and sync checks), follow
`evals/mesh-two-dev/RUNBOOK.md`.

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

## Restart an existing build safely

Run `./build.sh dev-prebuilt --wait` (PowerShell: `.\build.ps1 dev-prebuilt --wait`)
directly, without a preceding `kill`. This command validates provenance and runtime
prerequisites while the current app remains running, then stops and launches dev.
A failed preflight preserves the running app. There is no separate `restart` verb.
Use `dev` when HEAD or build inputs changed; stale provenance is a refusal to reuse
the artifact. Do not reset HEAD or edit the provenance record to bypass it.

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

- **2026-10-15: DEV panics during service state construction after a host-port split.** A late `SessionState` extension was set twice, once by service construction and again by desktop setup. Keep initialization in service construction; extension-set failures log the exact type. Rebuild with `dev` and require setup to proceed past service state construction before live probes.

- **2026-10-15: DEV loads stale installed providers after a successful build.** The gateway releases its temporary build drive before launch; compiled workspace paths on that drive no longer exist. `dev-prep.mjs` passes the physical checkout as `CODETERM_DEV_WORKSPACE_ROOT` and resets `CODETERM_WEB_DIST` from that same root. Rebuild with `dev`, then verify the startup plugin paths name the physical checkout and the expected manifest version. Never repair this by copying installed plugins or keeping the build drive mapped.

- **`dev-prebuilt` refuses a recorded build commit that differs from HEAD** —
  saved provenance belongs to another source revision. Run the normal `dev`
  operation with `--notify-tab`; require its fresh build/health receipt before
  testing. Preserve the refusal and never rewrite provenance to bypass it.

> When you hit a dev-startup issue on any platform, add a dated entry below with: symptom, root cause, fix. Keep entries short.

If the failure is caused by the dev-run procedure itself, fix the procedure as
well as the immediate environment before declaring recovery complete.

### Desktop bootstrap and packaged fallback (all platforms)

- **2026-10-11: Vite loads but the desktop root never acknowledges readiness** — the resize owner uses `client:<id>`, while frontend checkpoint scopes reject colons. The frontend uses a distinct filename-safe checkpoint ID. Optional update initialization failures carry a closed reason and HTTP status to the DEV log and render App with swapping disabled.
- **2026-10-11: fallback logs `AssetNotFound("index.html")` with no module entry** — Tauri generates an empty asset provider in debug builds with `devUrl`. The shell supplies a snapshot of the prepared `dist/` to that provider; release continues to use generated embedded assets. Rebuilding `dist/` alone cannot repair the empty provider.
- **Recovery** — integrate the repair and let the instance owner restart DEV through its existing scoped launcher. Verify `prepared development fallback assets installed`, the exact Vite navigation URL, frontend initialization or its typed refusal, and `ready-accepted`. Verify the packaged fallback also reaches module entry and readiness, and confirm Vite hot reload still works. Workers leave DEV lifecycle control with the owner.

### DEV browser terminal diagnostics (all platforms)

- **2026-10-10: `Cannot arm missing DEV xterm` or `DEV xterm instance unavailable`
  in a DEV web terminal capture** — the browser serves the watched `dist/`
  bundle through `CODETERM_WEB_DIST`. A default `vite build --watch` uses
  production `NODE_ENV`, so `import.meta.env.DEV` is false. DEV-only
  diagnostics therefore gate on `DEV_BUILD` (`src/lib/devBuild.ts`), the
  compile-time `__CODETERM_DEV__` that `CODETERM_FRONTEND_DEV=1` sets in
  `vite.config.ts`, never on `import.meta.env.DEV`. The watcher stays a
  production-mode build: a 2026-10-10 switch to `--mode development` (React
  development build) grew the Windows WebView2 renderer by about 0.5 GB/min of
  native memory. Release `pnpm build` leaves the flag false.
- **Recovery** — after canonical integration, restart only the affected DEV
  through its existing scoped launcher (`./build.sh kill`, then `./build.sh dev`;
  Windows uses `build.ps1`). Preserve any named-instance/port settings. Confirm
  the watcher finished a new bundle in `/tmp/codeterm-web-watch<SUFFIX>.log`,
  reload that DEV browser, select the exact fixture in Terminal mode, and verify
  its xterm host has `__codetermTerminal`. The desktop alone is not proof of the
  browser bundle. Isolated Workers defer this lifecycle and live check to the
  Orchestrator.
- **Wrong `selectedTabId` in capture metadata** — hidden tab roots remain
  mounted and can have client rectangles. Pass the fixture's exact DOM
  `data-tab-id`, including any mesh prefix, to the shared capture helper.
  It requires one matching root with visible CSS and positive geometry;
  a missing or hidden fixture fails instead of falling back to General Agent.
  Select that fixture on the intended peer before retrying in a fresh evidence
  directory; retain the failed run.

- **2026-10-10: web capture cannot click an existing fixture row** — its
  workspace is inactive, so the row is hidden. Copy the fixture inventory's
  `workspace_id` into that fixture's optional manifest `workspaceId` field.
  The shared selector activates that workspace through its visible Select
  workspace UI control, observes the selected state, then clicks the visible
  row. Hidden controls fail explicitly; never force-click.
  Verify the selected workspace and exact fixture before retrying in a fresh
  evidence directory, preserving the failed run.

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
- anything that implies running or diagnosing the local Domios dev instance

Don't preemptively run `./build.sh dev` on every edit — only when the user asks or when you have a Rust/prompt change that definitely needs a live binary to verify.

<!-- revision: 7 -->

### REST listener bind recovery

When the local socket works but HTTP is down, run `codeterm rest-api status` on the owning instance. A `bind_ownership_failed` diagnostic includes the address and OS error. Release the foreign port owner, then run `codeterm rest-api restart`; it waits for health proof and fails if binding still fails. These commands never fall back to HTTP or cross mesh. Confirm the actual Browser surface renders after Retry surface lookup. Never kill production to repair a listener collision.

Fixed: checks queue silently behind a running DEV. The former Tauri launcher held
the gateway for the app lifetime; update and rebuild with `dev`. Confirm
`./build.sh gateway-status` is idle while DEV runs, then run `check-queue`.

Fixed: standalone `dev-prebuilt` refused an app artifact path after a gateway build
recorded its Windows mapped-drive alias. The existing provenance writer and
validator now use physical artifact paths, including the embed and frontend, so
the record survives gateway mapping release. Restart directly with `dev-prebuilt`;
its preflight runs before stopping the app.

## DEV standard handles

Both foreground DEV and detached `--wait` use `scripts/dev-app-process.mjs`.
On Windows, `windows-dev-app.ps1` hosts the small .NET process boundary in
`windows-dev-app.cs`: .NET Framework redirects app stdout/stderr through
synchronous anonymous `CreatePipe` handles. Dedicated readers drain both streams;
only the wrapper connects to Node/libuv pipes. On Unix, the app writes directly
to a launcher-opened regular file, observed through file-change events.
The shared reader keeps the log and forwards READY signals in either mode.
Windows open-file notifications can be deferred until writer close, so they
cannot serve as the live READY signal there. `build.ps1` still only forwards.

| Symptom | Cause | Recovery |
|---|---|---|
| Windows DEV aborts with `fatal runtime error: I/O error: operation failed to complete synchronously` (event 1000, `0xc0000409`) | The former launch path gave Rust stdio an overlapped pipe. A synchronous write can return `ERROR_IO_PENDING` under reader load. | Rebuild with the corrected launcher and restart DEV. Keep Windows app stdout/stderr on the .NET synchronous-pipe boundary in foreground and `--wait`. Never attach app stdio directly to libuv or PowerShell pipeline pipes. |

Fixed: DEV recompiled crates from the physical `C:` checkout despite an active
`X:` gateway mapping. The old `dev-prep --no-server` calls duplicated CLI/embed
Cargo builds with a physical cwd, including one call after gateway release.
DEV now stages the CLI already built by `build.sh`; prebuilt launch performs no
Cargo preparation. Main app and sidecar compilation stay at `X:` until build
completion, then the mapping is released before launching the app.
