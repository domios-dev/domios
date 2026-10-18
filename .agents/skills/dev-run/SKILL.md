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

Builds the CLI, independent `domios-server`, and native client once each in the
existing dev profile, then launches the client through `dev-prebuilt`. The client
uses protected launch/attach to the detached server; it creates no ServiceState.
Windows resources live beside the actual native executable: `server-component.json`,
`domios-server/domios-server.exe`, exact paired CLI bytes, `dist/`, `skills/`,
licenses and ConPTY. The existing generator produces schema1/protocol2 inventory
after the final client bytes exist. This DEV layout is not a signed release layout.
macOS's authenticated installed wrapper locator remains unavailable; do not claim
a successful Windows DEV run proves other platforms or installer behavior.
DEV installs and bundles that exact debug CLI artifact. Production builds and
standalone `cli` retain their release profile. The CLI's existing package-only
opt-level 1 is unchanged; dependencies keep their existing profile settings.

Vite `build --watch` refreshes the staged resource `dist/` consumed by both the
server browser surface and the native DEV static origin; Rust edits need another `dev`.
For browser UI/provider development, `pnpm dev` uses Vite HMR. Its watcher
excludes Rust `src-tauri/target`, `cli/target` and `target-check` trees so native
compiler outputs do not inflate the frontend watch set. The loader accepts
the replacement module so providers use fresh glob imports. Do not switch the
default desktop watcher to React development mode: its WebView memory growth
is why this workflow uses production-mode bundles.
Installed JavaScript plugins reload on filesystem events without following
dependency symlinks, including creation
of a previously missing plugin root. Settings changes update the existing VM;
a failed code reload retains the last working plugin. Native Rust has no stable
hot-swap ABI here: use `check` and focused tests, then rebuild/restart dev.
A native hot-swap extension would need an explicit module boundary: keep tabs,
PTYs, sessions and service state in the host, and reload a pure parser or
transform over versioned byte inputs/outputs. Replacing arbitrary Rust libraries
cannot preserve the layout and lifetimes of trait objects, futures and globals.
A future implementation must retain each loaded generation until its in-flight
calls finish, use copied library filenames on Windows, and reject ABI mismatches.

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

Agent launchers for dev, build, check, QA batch and install-local (PowerShell and bash lines with automatic owner receipts): [utility-shell-reports](../utility-shell-reports/SKILL.md#launcher-recipes-copy-paste). Manual live drills: `scripts/hand-tests/README.md`; PowerShell isolation check against a warm dev: `scripts/check-dev-instance-isolation.ps1`.

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

Inside Domios, `dev` and `dev-prebuilt` resolve the recipient from the live
shell tab and report once when health is ready, or once on a startup failure.
An explicit `--notify-tab` overrides that route. The shared stream reader
covers setup and compilation; foreground readiness consumes the live pipe.
See the reporting skill above for parent, self-notification and mesh routing.

## Verifying the running binary matches your code

`build.sh kill` guarantees the old dev is dead before `build.sh dev` starts the new one — so any `/api/health` response after restart is from the fresh build.

When running from inside a production Domios tab, the shell may inherit
`CODETERM_API_PORT=7685`. `build.sh dev`, `build.sh kill`, and e2e commands
ignore inherited connection selectors and select canonical DEV on 7686. Explicit
launch settings use `CODETERM_DEV_INSTANCE` and `CODETERM_DEV_API_PORT`;
the launcher exports the selected `CODETERM_INSTANCE`/`CODETERM_API_PORT` to children.
It clears parent URL, token, socket and data-directory selectors after capturing
the parent report route. Optional frontend/CDP launch overrides use
`CODETERM_DEV_VITE_PORT` and `CODETERM_DEV_CDP_PORT`.

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

## Waiting for readiness

Use `./build.sh dev --wait` for an automation run that exits after health is
proven. It waits for the client's finite nonsecret `DOMIOS_DEV_ATTACHMENT` event after
exact authenticated server bootstrap and native enrollment, then validates
`/api/health`; a bounded deadline is a failure condition. Do not add a polling
loop or fixed sleep. For an interactive run, use one visible foreground dev
tab; its stream reader reports health readiness to the resolved Domios target.

If a build takes longer than the caller's command deadline, raise that
deadline. The build itself retains its bounded readiness failure deadline.

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
| `src-tauri/src/**/*.rs` | `./build.sh kill && ./build.sh dev` in a Team dev shell; use its READY receipt, then verify health once |
| `src-tauri/prompts/**/*.md` | same as above — included via `include_str!` |
| `cli/src/**/*.rs` | `./build.sh cli` — no dev restart needed |
| `src-tauri/Cargo.toml` deps | `./build.sh clean && ./build.sh dev` |
| `package.json` deps | `bun install` (or npm), then dev restart |

## Restart an existing build safely

Run `./build.sh dev-prebuilt --wait` (PowerShell: `./build.ps1 dev-prebuilt --wait`)
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

- **2026-10-18: named DEV cannot find its executable, or a second instance cannot replace a running Windows executable.** Interactive named builds previously used the default checkout target while `dev-prebuilt` looked under the instance data directory. Every named `dev` now builds CLI and app under `<checkout>/src-tauri/target/dev-instances/<instance-key>`, for both foreground and `--wait`; `dev-prebuilt` resolves the same checkout target. Config, installed CLI and the provenance record remain under `~/.codeterm-dev-<instance-key>`. Run `dev` once with the same explicit instance and private port to create the new artifacts and provenance, then reuse them with `dev-prebuilt`. Separate instance targets keep a second instance from overwriting the default instance's running executable. Cleanup remains scoped to the selected instance; never stop production to unlock a file.

- **2026-10-18: worktree DEV selected production port 7685.** The debug API default accepted inherited `CODETERM_API_PORT`, and a saved `[api].port` overrode that default. Startup refuses a saved production port even with a safe environment override, then selects the explicit dev launch port over safe saved config; TCP and mesh UDP binding also guard it. `dev`, `dev-prebuilt`, and scoped cleanup refuse non-canonical worktrees without an explicit instance and private port. In Git Bash use `CODETERM_DEV_INSTANCE=my-lane CODETERM_DEV_API_PORT=7799 ./build.sh dev`; in PowerShell set `$env:CODETERM_DEV_INSTANCE='my-lane'; $env:CODETERM_DEV_API_PORT='7799'` before `./build.ps1 dev`. Choose an unused port other than 7685/7686 and use the same settings for `kill`. If the refusal names the saved config, change `[api].port` in `~/.codeterm-dev[-<instance>]/config.toml` to that private port first. Canonical launch ignores parent connection variables and selects 7686. Explicit `CODETERM_DEV_API_PORT=7685` still refuses. Never stop production to resolve the refusal.

- **2026-10-17: DEV retains an older installed system provider.** DEV skipped embedded bootstrap, leaving installed bundles stale; workspace priority also ignored a newer installed release. Startup now refreshes bundled system ids with the existing version/hash installer, and both initial loading and the watcher keep the newest system version (workspace wins equal versions). Check the final `[js-plugin] codex version=...` startup line or `daemon-reloaded` audit event; an earlier `loaded ...` line or installed manifest alone does not identify the active implementation. After integrating the host repair, the owner rebuilds DEV once with `./build.ps1 dev`. To refresh a running DEV from the official channel without rebuilding, run `source evals/_lib/lib.sh` then `dc plugin update-system codex` in Git Bash from canonical. This installs the channel release and acknowledges live registration; it does not publish unreleased checkout changes. Never copy plugin files manually or downgrade a newer installed release.

- **2026-10-16: utility verbs abort while capturing identity outside a Git checkout.** Identity lookup ran under `set -e` before verb dispatch. Capture now tolerates unavailable Git and clears inherited stamps; stamping launches still refuse without a valid identity. Use the corrected script for utility operations; run builds from a Git checkout.

- **2026-10-16: DEV keeps showing the newer-interface pill after reload.** A commit during compilation let the internal `dev-prebuilt` re-entry recapture HEAD for the web watcher after Rust had stamped the earlier commit. `build.sh` now freezes full and short identities once and passes them to Rust, frontend builds, readiness checks and the internal provenance handoff. Standalone `dev-prebuilt` still refuses provenance from a different HEAD. Recovery: the instance owner rebuilds through `dev`, preserving its instance and port settings, then verifies health and `/build-identity.json` identify the same launch. Reloading the mismatched bundle alone cannot repair it.

- **2026-10-15: DEV panics during service state construction after a host-port split.** A late `SessionState` extension was set twice, once by service construction and again by desktop setup. Keep initialization in service construction; extension-set failures log the exact type. Rebuild with `dev` and require setup to proceed past service state construction before live probes.

- **2026-10-15: DEV loads stale installed providers after a successful build.** The gateway releases its temporary build drive before launch; compiled workspace paths on that drive no longer exist. `dev-prep.mjs` passes the physical checkout as `CODETERM_DEV_WORKSPACE_ROOT` and resets `CODETERM_WEB_DIST` from that same root. Rebuild with `dev`, then verify the startup plugin paths name the physical checkout and the expected manifest version. Never repair this by copying installed plugins or keeping the build drive mapped.

- **2026-10-16: `dev-prebuilt` refuses with `frontend artifact content mismatch` right after a successful `dev`.** `dev` hashed whatever `dist/` a previous watcher session left (an older HEAD's build identity), then the prebuilt watcher rebuilt `dist/` for the current HEAD, so the record no longer matched the served bundle. `dev` now runs one `CODETERM_FRONTEND_DEV=1 vite build` (`build_dev_frontend`) before `write_dev_provenance`; the watcher's same-HEAD rebuild is byte-identical, so the next `dev-prebuilt` matches. Preflight output and refusals are appended to `/tmp/codeterm-dev-prebuilt<suffix>.log`. Recovery on an older checkout: run `dev` once; a refusal after that is a genuine mismatch (edited `dist/` or sources), so never rewrite the record.

- **2026-10-16: the DEV WebView shows only `dist/index.html missing` after a same-SHA restart.** That text is the 404 body of `scripts/dev-static-server.mjs`. The prebuilt `vite build --watch` emptied `dist/` at the start of its first rebuild (~12s) while the static server and WebView were already up, so the first navigation hit the empty directory. The watcher now runs with `--emptyOutDir false` and overwrites in place; `dev` still builds a clean `dist/` before recording provenance. Recovery on an older checkout: reload the window once the watcher log shows `files generated`, then restart through the fixed `dev`.

- **`dev-prebuilt` refuses a recorded build commit that differs from HEAD** —
  saved provenance belongs to another source revision. Run the normal `dev`
  operation; require its fresh build/health receipt before
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
- **2026-09-30: first named `dev --wait` exits 1 silently after bundle preparation** — `record_dev_target` ran `cd` into `<instance>/target/debug`, which a fresh named target does not have yet (only `release/`), and `set -e` aborted without a message. `record_dev_target` now creates the directory; on an older checkout, `mkdir -p ~/.codeterm-dev-<name>/target/debug` and rerun.
- **`~/.codeterm-dev/mem/` corrupted / mem commands 500** — back it up and restart: `mv ~/.codeterm-dev ~/.codeterm-dev.bak-$(date +%s) && ./build.sh dev`.

### Windows

- **Slow MSVC link step** — the project Cargo config selects Rust's bundled
  `rust-lld.exe` for x86_64 MSVC. It still needs the Windows SDK and MSVC
  libraries loaded by build.sh. If the linker cannot be found, repair the
  installed Rust toolchain; do not replace it with a machine-specific path.
  Use the build skill's `dev-link-measure --wait-build` recipe to separate
  linker time from gateway/setup time without restarting the running DEV.

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

<!-- revision: 8 -->

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

### Independent DEV launch troubleshooting

A missing `server-component.json`, wrong CLI binding, or stale server executable
must refuse before client launch. Use the supported `dev` build/stage sequence;
never manufacture hashes or reuse the process-probe resource directory. The
schema3 prebuilt provenance pins server, inventory and runtime-resource bytes in
addition to client/CLI/frontend bytes. A monolithic REST log cannot satisfy its
attachment readiness event. Optional native enrollment failure may leave the
headless service alive but does not emit successful native DEV readiness.

The server's compiled debug DEV frontend origin is the selected loopback static
port; production selectors ignore it. There is no frontend paint prerequisite
for service readiness. Titlebar close must preserve the exact server/session;
explicit Quit requires the protected protocol2 cleanup reply plus pinned exit.
Those are parent-owned native proof rows, not implied by staging unit tests.

If the native client shows the unavailable document with no server/listener, a
pre-split server log is not current client evidence. Debug client stderr writes
at most 64 sanitized `DOMIOS_CLIENT_DIAGNOSTIC` records (fixed stages, IO kind,
numeric OS/helper exit codes only). `ERROR: DOMIOS_DEV_FAILURE` is a finite
terminal launch report; the supervisor rejects it immediately without extending
the deadline or trying another launch. Inspect the last install/attach/launch
boundary and exact nonsecret code, never helper stdout or API credentials.
Helper stderr remains hidden; this diagnostic does not authorize another retry
or imply that a previously timed-out server/client launch became ready.

### Windows detached server refused by an older utility job

Symptom: verified DEV artifacts reach launch, then human start reports Access denied
(os error 5). A PTY job without explicit breakaway permission can reject the server's
CREATE_BREAKAWAY_FROM_JOB request. Confirm the actual spawn boundary before attributing
an arbitrary permission failure to jobs.

The host admits explicit breakaway only for persisted Team Dev utility shells. Their
job retains KILL_ON_JOB_CLOSE; ordinary descendants remain contained. Default, agent,
non-Team and other-purpose shells cannot request that policy. No silent breakaway or
runtime environment switch exists. The server still refuses residual ancestor-job
membership through ensure_detached.

Activation requires a reviewed host carrying this policy to create a NEW utility PTY.
A newly built DEV client cannot retrofit jobs created by the older installed host.
Do not mutate a live PROD job, restart/install PROD automatically, or relax containment.
An outside-job parent can separately verify exact staged artifacts, but cannot pass the
Team utility activation oracle. Parent QA must observe default/agent/ordinary Dev
children die on job close and only the explicit server survive with its retained
process pin, authenticated epoch and existing session unchanged.

### Unix DEV raw artifact/resource directory collision

Cargo produces a regular `target[/<triple>]/debug/domios-server` file. Placing paired
`domios-server/` resources in that directory fails with ENOTDIR. DEV stages one exact
copy of the built client under `.domios-dev-runtime/bin/codeterm` and paired resources
under `.domios-dev-runtime/lib/codeterm`, retaining raw Cargo outputs. Linux Tauri's
existing `exe_dir/../lib/<package-name>` lookup resolves that resource root; no fake
Cargo lock, CWD or environment resource override is used. Windows layout is unchanged.
The provenance record, watcher and static server use that same derived resource root.
macOS authenticated installed inventory location remains Unsupported; staging is not
proof of macOS startup. Parent native Linux proof must verify actual resource lookup,
exact staged hashes and authenticated attachment for host and target-qualified output.

### Automatic Windows DEV launch from an older host job

`build.ps1 dev` uses the existing `build.sh` build/stage operation once, then its
Windows DEV launcher asks the current user's Explorer desktop automation object to
start a finite hidden execution helper. This is DEV-only; no host install/restart,
elevation, scheduled task, global job edit or general broker is involved. An absent
interactive desktop, wrong user/session, remaining helper job membership, invalid
staged client or failed private pipe refuses launch. COM return is never readiness.

The operation pipe permits only the current user, has bounded input/output frames,
and transmits only whitelisted nonsecret DEV settings. Client bootstrap/API credentials
never enter launcher argv/log/files. The native client still verifies component bytes,
authenticates and pins its server; existing authenticated attachment supplies readiness.
The helper owns the native client handle. Controller cancellation/disconnect terminates
that client only; supported scoped `build.sh kill` remains the DEV server cleanup path.
Titlebar close/tray lifetime differs from explicit controller cancellation. Parent QA
must verify actual desktop launch, exact pair/renderer ownership, titlebar/session
survival and protocol2 Quit cleanup. Source fixtures do not prove those native rows.

### Gateway drive spelling refused during DEV staging

Symptom: all builds finish, then `stageDevResources` refuses the derived resource
root before desktop launch. The gateway's SUBST drive can spell the same artifact
root differently from the canonical client path. Staging resolves the existing
root (or nearest existing parent for missing Unix layout directories) before comparing
it to the exact canonical client-derived root. Symlink/junction and foreign-root
candidates still refuse. Only the verified physical root is created/staged; no global
drive mapping is changed. Rebuild through the supported DEV operation after this
source correction; COM/client/server startup remains a separate native proof.
