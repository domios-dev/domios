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

## How we run DEV: two tabs

DEV runs as two independent processes so every change updates live without losing the
window: frontend edits hot-reload in place, server edits swap the backend while the window
shows Reconnecting and returns, and only native client changes restart the window.

| Tab (purpose `dev`, no timeout) | Command | Owns |
|---|---|---|
| `domios-front` | `./build.sh dev-front` | Vite dev server with HMR on 127.0.0.1:1420, proxying `/api`, `/frontend`, `/bundled` to DEV |
| `domios-back` | `./build.sh dev-back` | Build, `domios-server` and the native window; uses the front tab's frontend |

Start `domios-front` first; the window shows the branded loader and retries until it
answers. Restarting either tab never stops the other. Plain `./build.sh dev` still runs
everything in one tab with the production-mode bundle watcher.

A Domios restart (self-install, daemon crash) ends both tab commands. Restart `domios-front`
before `domios-back`: with Vite down the DEV window keeps Reconnecting, re-attaching every few
seconds (the server log repeats `native integrity hash` while `client-startup.log` cycles
through `native_enrollment`), because its configured development frontend refuses the probe.
Prod has no development frontend, so the same build reaches the interface there.

`dev-back` launches the window detached and returns once it is ready. On Windows the launch
leaves the tab's kill-on-close job (Team `dev` tabs allow it), so a `domios-back` restart keeps
the window. A rerun with a live window builds beside it: the window locks its executable, so a
client link that succeeds proves the client unchanged and only the server swaps; a lock failure
replaces the window. Hosts older than the Team DEV breakaway report that the window stays tied to the tab.

The Windows interactive helper may inherit an Explorer/session job. Like the server,
it requires readable job limits without `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, rather
than absence of all jobs. A kill-on-close or unreadable job refuses before client
launch. Inspect helper job limits when diagnosing this refusal; Team DEV breakaway
policy does not change Explorer's job membership. No production restart is needed
for a launcher-script repair.

Bench helpers sourced from `evals/_lib/lib.sh` select frontend/CDP ports from the
matching schema3 launch provenance, including explicit launch overrides. Select
the named instance with `CODETERM_INSTANCE` and `CODETERM_API_PORT` before sourcing;
the helper still verifies the selected instance, build SHA and ports against health
and provenance before any authenticated request.

With `dev-back`, native attachment has a 180-second failure deadline after reply validation
to allow Vite's unbundled modules and large restored workspaces to load. Progress events
advance the startup phases; only the native attachment acknowledgement and server readiness
events prove readiness. If a phase expires, the command returns exit 3 with “still attaching”
and leaves the server and window running, including when server health is already ready.
Plain `dev` keeps its existing deadlines and cleanup behavior.

After exit 3, inspect the printed prebuilt log and `client-startup.log`, confirm that
`dev-front` is running, and let the existing window complete attachment. Server health alone
does not prove the window attached. Do not rerun `dev-back` just to extend its wait; if the
window cannot recover, the DEV owner may explicitly stop the selected instance with
`./build.sh kill` and restart `dev-front` before `dev-back`.

## Firebase emulator DEV

Start and seed the Firebase Emulator Suite using the sibling repository's
[LOCAL-DEV.md](../../../../codeterm-firebase/docs/LOCAL-DEV.md) (published source:
[codeterm-firebase/docs/LOCAL-DEV.md](https://github.com/rollacode/codeterm-firebase/blob/master/docs/LOCAL-DEV.md)).
It uses the synthetic `demo-domios` project, Auth `19099`, Firestore `18080`,
Functions `15001` and Hosting `15000`, all on `127.0.0.1`.

Set the explicit runtime opt-in in the DEV utility shell before launching the
debug server. Keep the normal named-instance selectors for a noncanonical lane:

```powershell
$env:DOMIOS_FIREBASE_EMULATOR = '1'
$env:CODETERM_DEV_INSTANCE = 'firebase'
$env:CODETERM_DEV_API_PORT = '17686'
bash ./build.sh dev
```

For Bash, export those same three variables before `./build.sh dev`. The opt-in
selects Firebase even with a saved local account provider and uses a fake API
key; no production credentials are needed. Start sign-in from the desktop so
the browser receives its state and PKCE challenge. Hosting serves
`http://127.0.0.1:15000/signin/`; redemption and billing call
`http://127.0.0.1:15001/demo-domios/us-central1/`. Identity Toolkit and securetoken
use their respective API paths beneath `http://127.0.0.1:19099/`.

Use the seeded identities/passwords from LOCAL-DEV.md. Log out before switching
between emulator and production targets; saved refresh credentials belong to
their Auth service. Unset the opt-in and restart DEV to restore its ordinary
configuration. Emulator HTTP is admitted only for these exact service paths
and ports with the opt-in enabled.

Release builds compile in production Auth, `auth.domios.dev` sign-in and
`us-central1-domios-d9357.cloudfunctions.net` functions. Runtime config and
endpoint environment variables cannot redirect release builds. The emulator
endpoint table and DEV grant activation are excluded from shipped release
builds by `cfg(debug_assertions)`. DEV with the opt-in can verify the existing
public RFC 8032 fixture key for `demo-domios`, test mode and epoch
`fixture-online-v1`; Phase A keeps the existing envelope access behavior.
The emulator backend currently returns the existing envelope without a signed
grant, so local sign-in/entitlements do not establish a signed-grant round trip.
Production `ACTIVATION` remains dormant (`None`) until an owner-reviewed epoch
and production public key are pinned in source. Release never selects DEV trust.

## Measuring the live window

`scripts/dev-perf/` attaches to the DEV webview over CDP (`CODETERM_DEV_CDP_PORT`, default
9223; never prod) and measures what the user feels:

| Command | Answers |
|---|---|
| `node scripts/dev-perf/typing.mjs --prompt "Write a long poem"` | Keystroke-to-frame latency at rest and while the active agent answers, React commits, which components rendered and why (context or hook) |
| `node scripts/dev-perf/connections.mjs --seconds 30` | Daemon requests held open by the page; about six per origin starve everything else |
| `node scripts/dev-perf/profile.mjs --seconds 10` | CPU self time and app-frame totals |
| `node scripts/dev-perf/open.mjs http://127.0.0.1:<vite>/ --port <cdp>` | Opens a lane frontend as a DEV web client in a Domios Browser (`domios plugin browser endpoint`) with the hook installed |
| `node scripts/dev-perf/surface.mjs <scenario> --port <cdp> --url-match 127.0.0.1:<vite>` | One surface driven by real wheel, pointer, touch (`--width 390`) or key input: renders per component, long tasks, the hook or context behind each render and the props that re-rendered a memoized component |
| `node scripts/dev-perf/suite.mjs ... --out run.jsonl` then `compare.mjs before.jsonl after.jsonl` | Every surface in order, and the before/after table |

Activate a cheap agent tab first (Haiku, Luna). `typing` and `connections` reload the page
once to install their hook before React loads. The Vite dev server hides `console.timeStamp`
so React's dev-only prop-diff logging does not distort these numbers.

For a lane frontend, run its own Vite against DEV (`CODETERM_API_PORT=7686 CODETERM_VITE_PORT=<free port> node node_modules/vite/bin/vite.js --port <same>`) in a dev utility tab and open it with `open.mjs`. Keep cheap agents streaming in another workspace for load, and compare before and after in one page alternately under the same load: a hidden tab is throttled and measures nothing.

## Reload by change type

| Changed inputs | Command |
|---|---|
| Frontend `src/`, styles or `public/` | Nothing; `domios-front` hot-reloads the module in place (single-tab `dev`: the bundle watcher rebuilds and the window reloads) |
| Server crates, embedded server prompts, server plugins | `./build.sh dev-server` |
| CLI or its local dependencies | `./build.sh dev-server` rebuilds the CLI when its sources changed |
| Native client `src-tauri/src/` (including `host_ports`, `startup/frontend_shell`), client identity/runtime crates, Tauri config, build configuration | `./build.sh dev` |
| First launch or missing/stale provenance | `./build.sh dev` |
| Reuse an unchanged build | `./build.sh dev-prebuilt --wait` |
| Stop this selected instance | `./build.sh kill` |

`dev` builds CLI, independent `domios-server` and native client, records schema3
provenance, releases the build gateway, then launches through `dev-prebuilt`.
`--wait` returns after authenticated native attachment and exact health proof.
Foreground launch stays in its Dev shell and owns the client process.

Server reload/staging tool updates also use `dev-server`; native source and
Tauri/Cargo configuration changes still require `dev`. Named DEV CLI artifacts
build into the instance target's `cli/` child so the native build cannot overwrite them.

`dev-server` validates the existing instance/artifacts and classifies current
bytes against the last build snapshot, including uncommitted edits and deletions.
Client/build changes refuse with `run ./build.sh dev instead` before shutdown.
It compiles only the server and changed CLI sources while the old server runs,
verifies the new build/CLI binding, then uses authenticated protocol2 shutdown
and a pinned process-exit observation. It stages the new server/inventory,
refreshes provenance and launches the same instance, config, data and API port.
The client process/window, static frontend server and watcher stay alive.
A shutdown uncertainty refuses; it never force-kills or retries the swap.
Windows publishes server replacements by rename; pinned old images remain as
`.retired` artifacts under the target directory until target cleanup.

The native window shows branded Reconnecting while detached. Its existing
10-second recovery verifies the current DEV inventory, reattaches to the new
epoch and navigates the same window to the interface. Recovery never replays
input or launches an old server while a server swap is in progress.
Server READY is authenticated server proof; confirm the actual native UI and
restored tab IDs separately. Frontend paint is not implied by public health.

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
node scripts/reap-dev-sidecars.mjs "$HOME/.domios-dev" --count
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

- **macOS/Linux:** `~/.domios-dev/logs/domios.log`
- **Windows:**    `%USERPROFILE%\.domios-dev\logs\domios.log`

Tail live:

```bash
tail -f ~/.domios-dev/logs/domios.log
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
`dc config get api_listener.ready_addresses` before `dc mesh link`.
Removing the opt-in uses JSON `null` and another scoped restart.

To join two DEV consoles on different machines over the mesh (pairing,
managed-by tabs, web UI peer picker, leak and sync checks), follow
`evals/mesh-two-dev/RUNBOOK.md`.

A failed secondary bind starts neither listener; no wildcard or LAN fallback
is allowed. A later secondary listener exit keeps local HTTP serving but clears
remote readiness. Recovery is an explicit scoped restart,
not a retry loop. Preserve a failed trial's state and distinguish a new trial.

Troubleshooting: `mesh_listener_not_remotely_ready` means no health-proven remote
listener can serve the announcement. A configured host is not live evidence.
Do not widen the default bind or repeatedly link to a stale endpoint;
verify the named instance's live listener set first.

- `~/.domios-dev/` — config, mem DB, sessions, logs, mesh identity
- `~/.domios-dev/config.toml` — themes, keybindings, api section
- `~/.domios-dev/mem/` — mem SQLite + FTS + vectors
- `~/.domios-dev/sessions/` — persisted workspaces/tabs

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
mv ~/.domios-dev ~/.domios-dev.bak-$(date +%Y%m%d-%H%M%S)
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

- **2026-10-18: named DEV cannot find its executable, or a second instance cannot replace a running Windows executable.** Interactive named builds previously used the default checkout target while `dev-prebuilt` looked under the instance data directory. Every named `dev` now builds CLI and app under `<checkout>/src-tauri/target/dev-instances/<instance-key>`, for both foreground and `--wait`; `dev-prebuilt` resolves the same checkout target. Config, installed CLI and the provenance record remain under `~/.domios-dev-<instance-key>`. Run `dev` once with the same explicit instance and private port to create the new artifacts and provenance, then reuse them with `dev-prebuilt`. Separate instance targets keep a second instance from overwriting the default instance's running executable. Cleanup remains scoped to the selected instance; never stop production to unlock a file.

- **2026-10-18: worktree DEV selected production port 7685.** The debug API default accepted inherited `CODETERM_API_PORT`, and a saved `[api].port` overrode that default. Startup refuses a saved production port even with a safe environment override, then selects the explicit dev launch port over safe saved config; TCP and mesh UDP binding also guard it. `dev`, `dev-prebuilt`, and scoped cleanup refuse non-canonical worktrees without an explicit instance and private port. In Git Bash use `CODETERM_DEV_INSTANCE=my-lane CODETERM_DEV_API_PORT=7799 ./build.sh dev`; in PowerShell set `$env:CODETERM_DEV_INSTANCE='my-lane'; $env:CODETERM_DEV_API_PORT='7799'` before `./build.ps1 dev`. Choose an unused port other than 7685/7686 and use the same settings for `kill`. If the refusal names the saved config, change `[api].port` in `~/.domios-dev[-<instance>]/config.toml` to that private port first. Canonical launch ignores parent connection variables and selects 7686. Explicit `CODETERM_DEV_API_PORT=7685` still refuses. Never stop production to resolve the refusal.

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
- **`caffeinate` stays running after dev crashes** — if a session used `domios keepawake start`, the caffeinate child is owned by the dev process and should die with it. If it doesn't: `pkill -x caffeinate` (safe — targets only the literal binary name).
- **Port 7686 already in use but no dev window** — stale process from a previous crash. `./build.sh kill` then start. If still occupied, `lsof -nP -iTCP:7686 -sTCP:LISTEN -t | xargs kill` and retry.
- **2026-09-30: first named `dev --wait` exits 1 silently after bundle preparation** — `record_dev_target` ran `cd` into `<instance>/target/debug`, which a fresh named target does not have yet (only `release/`), and `set -e` aborted without a message. `record_dev_target` now creates the directory; on an older checkout, `mkdir -p ~/.domios-dev-<name>/target/debug` and rerun.
- **`~/.domios-dev/mem/` corrupted / mem commands 500** — back it up and restart: `mv ~/.domios-dev ~/.domios-dev.bak-$(date +%s) && ./build.sh dev`.

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

<!-- revision: 13 -->

### REST listener bind recovery

When the local socket works but HTTP is down, run `domios rest-api status` on the owning instance. A `bind_ownership_failed` diagnostic includes the address and OS error. Release the foreign port owner, then run `domios rest-api restart`; it waits for health proof and fails if binding still fails. These commands never fall back to HTTP or cross mesh. Confirm the actual Browser surface renders after Retry surface lookup. Never kill production to repair a listener collision.

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

### Independent server is live but native helper reaches its deadline

Public health alone cannot complete native attachment. If launch has started but the
caller still refuses, inspect sanitized `response_newline`, `response_eof` and
`helper_exited` phases. Newline plus helper exit without EOF identifies an open writer;
server debug logs bracket native listener binding and inherited readiness ACK. No
payload, token or reply contents are recorded. Do not increase deadlines or retry attach.

Windows launcher standard handles are made non-inheritable before spawning the server,
which retains its explicit private stdin/stdout. This prevents those original helper
handles from extending the caller pipe lifetime. Exact bounded one-line plus EOF and
successful helper exit remain mandatory. The prior deadline is not reclassified as
success; parent native execution must establish the actual handle/ACK outcome.

### Authenticated split attachment succeeds but the reporter refuses health

The independent server owns full build SHA, instance name and API port. Client Vite,
CDP and window identifier values are not server health identity. Split reporting
requires exact server health plus the fresh native attachment event, matching API
listener PID and observed live client/server. The event comes from the verified
client's authenticated retained attachment; supplemental PID liveness is not a new
process-provenance verifier. Legacy combined-role health checks stay strict.

Launch configuration and staged client/frontend bytes remain pinned by DEV provenance.
Bench target checks accept empty server GUI fields only with the selected schema3
DEV provenance/build/instance record; existing CLI/data/config/socket isolation remains.
Neither launch reporting nor bench identity claims renderer paint, browser login or an
active CDP endpoint. Those require observations on their own selected client surface.

### Streaming replay inspection rejects blank server GUI properties

The streaming-soak consumer previously required server identifier/Vite/CDP fields and
forced external Browser targets onto Vite. Split DEV inspection now reads the selected
schema3 provenance and pins full server build/name/API identity, then admits only the
explicit Browser target at the API-served DEV origin. It hashes actual served HTML and
entry script bytes. Target/socket/renderer PID/incarnation/native-bridge refusals remain.
Legacy combined-role target checks stay strict. Split native inspection uses the
checked Windows debug locator described below; without it inspection remains unavailable
rather than inventing CDP from client configuration or blank server fields.
Parent Browser authentication and live fixture work remain separate authorized steps.

### Browser-wide renderer count blocks functional replay

The streaming-soak runner defaults to strict profiling attribution. For finite
input/storage correctness on an existing multi-renderer Browser, use its explicit
`--purpose functional` on inspect/setup/replay/finish/cleanup; follow REPLAY.md.
The same page/entry/build/boot/daemon and fixture journal remain pinned. Renderer
PID/private bytes/trace attribution are explicitly unavailable, never guessed.
Functional reports retain DOM/input/storage and page-CDP metrics, refuse profiling,
heap snapshots and memory/plateau comparisons, and cannot resume across capability
or page drift. No other Browser page/profile is changed to obtain a renderer PID.

### Bundled functional replay cannot import TypeScript source paths

The API-served dist returns HTML for `/src` imports. Functional replay now preflights
the same owned page at existing local DEV `?e2e` and requires the shipped test-chat
and projection handles BEFORE setup fixtures. Re-inspect/re-pin that changed page;
restore its saved URL only after exact cleanup. No source module copies/app hooks or
Vite/HMR switch are used. Backend fixture identity remains checked, while public
projection/DOM tail observations explicitly do not prove internal rows/authority or
durable storage. Use the bounded procedure in streaming-soak/REPLAY.md; unavailable
handles or unmounted synthetic projections refuse rather than fabricating completion.


Bundled functional replay selects the exact owned workspace and tab on the selected
Browser page through sidebar controls; daemon focus is only used for the native
surface. Backend focus acknowledgement alone does not prove a mounted composer.
Functional cleanup uses authenticated mux reads after each successful close and a
final all-owned-IDs-absent read before unlinking fixture files. UI absence remains
unavailable; profile mode retains its source bridge event observer.
For a pre-close cleanup failure with pending null and cleanup null, run only the
original checkpoint's `cleanup` with its original purpose/surface/CDP/target flags.
Do not rerun setup or replay that checkpoint. After exact cleanup, use a fresh
output directory for inspect/setup/replay/finish/cleanup and restore the saved URL.


A bundled replay workspace may be outside the collapsed sidebar card subset.
The harness expands the unique visible overflow control once only when the exact
owned workspace control is absent, then observes that exact control before clicking.
Missing-after-expansion times out/refuses; duplicate controls refuse. This is not
proof that overflow caused a prior refusal when fixture membership was not captured.
Tab activation uses the actual TabItem aria-current="page" contract plus a visible
owned mounted composer/terminal; backend focus alone remains insufficient.


### Unfinished replay: explicit teardown only

Ordinary finish/cleanup/resume refuse a pending replay. After owner approval use
`bash evals/perf/streaming-soak/run.sh abort-replay --out ORIGINAL_RUN --purpose functional --surface chrome --cdp EXACT_CDP --target EXACT_PAGE`.
Keep the original page/e2e/build/entry/daemon identity. This operation validates
exact session/nonce/PTY/workspace and fixture files, admits one abort, authenticates
the existing emitter stop and requires its exact child-exit receipt. It then closes
only owned tabs/workspace, proves backend absence, and unlinks owned fixture files.
Pending and replay failure evidence remain unchanged; a separate abort record and
aborted stage never count as replay success or permit replay/resume. An interrupted
abort/cleanup needs owner review, never automatic repeat. Do not clear a pending
journal, seed again, erase its checkpoint or create another run before teardown.


Functional replay now visits each exact seed target before dispatch/visible-tail
verification, returns to local before measured input, and selects each target for
final restoration/read. Inactive mounted chat shells may contain no message DOM.
Measured background-control evidence therefore uses only the public projection tail;
its visible DOM/internal store/durable storage evidence remains explicitly unavailable.
Selected local and final selected-tail checks still require the exact visible source ID.
The actual tail signature is row id plus # plus JavaScript content length, matching
paintTailSignature; no monitor snapshot is converted into invented DOM rows.


Replay visits explicitly distinguish setup from replay phase. Initial authority
mount retains the original synthetic history marker. Seed routing, begin/local
return, final reads and after-return use replay phase: exact selected owned mount
remains required, while dispatch/final-read retain exact replay tail evidence.
The setup marker may disappear after normalized reseed/virtualization; requiring it
then caused a pre-input readiness refusal, not proof of app storage failure.


### Windows split native inspection locator

Supported dev/dev-prebuilt selects the existing instance-specific CDP port and
passes the finite local diagnostics option only to the Windows debug native client.
WebView2 binds loopback; release builds do not enable it through this path. No
server/CLI/PTY argument or credentials are forwarded. The existing client verifier
checks exact main-view generation and WebView process lineage before Active.
The launcher records the checked snapshot, origin, client PID/kernel creation identity
and view/navigation generation in schema3 provenance nativeClient after authenticated
attachment. Each new launch clears the old locator. Inspect refuses process creation,
executable, current native snapshot or view-target generation mismatch. The native
renderer attribution rules stay strict; functional mode still makes no memory claim.
macOS WKWebView and Linux WebKitGTK do not expose this Windows WebView2 CDP seam;
no locator is published there. Their native inspection remains unavailable.
Use the existing streaming-soak inspect --surface native after the parent rebuild;
no external Browser endpoint or guessed native renderer is substituted. The frozen
58a pair has no new locator and must remain untouched until coordinated rebuild.


### DEV native locator stays null despite its WebView2 listener

If the diagnostics argument and exact client-owned loopback listener exist but no
locator event appears, do not restart or infer the endpoint from the port. The old
publisher incorrectly reread CODETERM_DEV after install_client_selector scrubbed it.
Publication now uses the verified captured Development selector, with debug gating.
The root-render effect removes its temporary query after acknowledgement; native
snapshot inspection uses the exact acknowledged native latch and current attachment,
not that cosmetic query. Present stale queries still refuse. An absent query admits
only a unique target candidate; the native command snapshot plus exact process
incarnation must still match before inspection succeeds. No ambient selector is
restored, no client credential is exposed and no new frontend/runtime hook is added.


### DEV attach exits4, then the launcher deadline expires

Attach exit4 is the finite ordinary unavailable-endpoint outcome and permits one
initial launch; it is not an authentication/build failure. If launch starts but the
outer prebuilt readiness deadline expires, preserve server bootstrap ACK timestamps
and the client stream. The former outer30-second guard began before desktop spawn
and artifact verification, while the client's bootstrap clock began later. The
launcher now partitions preparation/bootstrap/attachment into bounded phases at
ordered native diagnostic boundaries. Only exact native attachment is readiness;
duplicated progress never extends a phase. Inner bootstrap and IO deadlines remain
unchanged; no retry or public-health fallback is introduced.

Named-target script fixtures must load dev_built_app_path alongside dev_app_path.
Windows uses the raw executable path; Unix keeps raw Cargo bytes and stages the native
client under .domios-dev-runtime/bin. The old fixture missed this c8a851cdd extraction;
its repair checks both platform layouts and wait modes, preserving the default artifact.

### Separate DEV verification and bootstrap budgets

A slow verified startup previously consumed the outer30-second budget before the client began its own bootstrap budget. Plain `dev` bounds preparation, bootstrap and authenticated attachment separately at30seconds each; `dev-back` uses the longer attachment deadline and retains processes on expiry as described under “How we run DEV: two tabs”. Only the ordered attach-start and reply-validation events move the boundary; duplicates never extend a phase. Progress never means readiness. Existing inner bootstrap/transport deadlines remain unchanged.

Debug `[boot-operation]` records measure embedded extraction, capability registration (including binary resolution/fingerprint), integrity hashing with actual bytes, restored plans and platform initialization. Capability timing includes the registration call rather than claiming a separate binary-probe duration; compare it with the existing VM model timing. No digest or private bootstrap reply is logged.

### Component schema 2 and macOS nested server identity

Component manifests now require schema2 and a platform-specific client_identity. Windows/Linux require the exact lowercase SHA256 variant; macOS requires the approved Apple-anchored team/identifier designated requirement. macOS resolves only ../MacOS/domios-server from Tauri's canonical Domios.app/Contents/Resources root. The client validates its running code identity and the installed app seal; the server validates the attaching pinned client the same way. Server bytes remain SHA256-pinned on every platform. A schema1 manifest, foreign identity kind, weak requirement or wrong bundle path refuses; rebuild/restage the matching pair rather than bypassing verification. No signed macOS runtime proof is supplied by source or Windows lane compilation. Native CDP remains Windows-only.

Headless schema2 manifests use client_identity kind none and the exact flat domios-server/domios-server[.exe] layout on every OS. The server verifies those bytes without creating a native-host listener; native client enrollment refuses none. macOS flat raw-client DEV staging remains unsupported. Named isolated DEV stages the signed app pair described below; debug builds retain the platform identity rule.

### Named isolated macOS DEV

Use an explicit unused instance and private API/frontend/CDP ports from the assigned lane:

```bash
CODETERM_DEV_INSTANCE=macdev-fix CODETERM_DEV_API_PORT=17991 \
CODETERM_DEV_VITE_PORT=14991 CODETERM_DEV_CDP_PORT=9491 ./build.sh dev --wait --wait-build
```

The launcher stages `<instance-target>/debug/.domios-dev-runtime/Domios.app`,
with the native client and signed server in `Contents/MacOS` and the component
inventory in `Contents/Resources`. Host and target-qualified Cargo outputs stay
unchanged. Tauri resolves `../Resources` from that client; no resource override
or fake Cargo marker is used. Canonical DEV, Windows/Linux and release paths
retain their existing layouts.

The normal local signing selector requires one Developer ID Application identity
in the login keychain; ambiguous or unavailable identities refuse. Explicit
`APPLE_SIGNING_IDENTITY`, `APPLE_TEAM_ID` and `RELEASE_KEYCHAIN` select an existing
approved identity. No ad-hoc fallback or notarization is used for this local DEV
bundle. The nested server is signed before inventory hashing, then the outer
app is sealed without `--deep` signing. Strict nested verification tests the
exact Apple-anchored team and `com.domios.app` requirement before provenance is
recorded; native verification still runs at launch.

Apple Silicon macOS enables the SHA crate's supported hardware backend for the
native client and server, including release builds. Full-file validation and
all bootstrap deadlines remain unchanged; Windows/Linux and Intel macOS keep
their existing SHA feature selection.

The production-mode frontend watcher and static server use the checkout's
`dist/`, outside the sealed app. The app's resource snapshot stays immutable.
Named macOS `dev-server` refuses before shutdown because replacing nested code
would change the seal beneath a running client; use the same scoped `dev`
command for native/server edits. `dev-prebuilt --wait` reuses an unchanged signed
pair with the same explicit instance/port settings.

Detached named macOS cleanup has a follow-up limitation: scoped `build.sh kill`
stops the server and watchers while the surviving native client may recover a
new server. For an owned validation run, retain the native attachment receipt,
exact executable and process creation identity; stop only that verified client
first. Then use `node scripts/dev-server.mjs inspect <record> <data> <private-port>
<owned-receipt>` and `node scripts/dev-server.mjs stop <record> <data>
<private-port> <owned-receipt>` for required cleanup and pinned server exit.
Finally use the same explicit instance/port with `./build.sh kill` to reap its
watcher/static processes and verify all owned processes exited. Native-client
shutdown automation needs a separate ownership-aware fix; never use global
process-name kills or address prod, canonical DEV or another instance.
