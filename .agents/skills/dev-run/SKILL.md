---
name: dev-run
description: Start, reload, stop and verify an isolated Domios DEV instance.
---

# DEV iteration

Never touch production (7685) or another owner's DEV. Default DEV uses 7686.
All build logic is in `build.sh`; PowerShell forwards with `./build.ps1`.
Run launches and reloads in a Team utility shell with `--purpose dev` and no
shell timeout. See [utility-shell-reports](../utility-shell-reports/SKILL.md).
Workers start DEV only with an exact live-proof assignment.

| Changed inputs | Command |
|---|---|
| Frontend `src/`, styles or `public/` | Nothing; the existing production-mode Vite watcher refreshes the staged bundle |
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

`dev-server` validates the existing instance/artifacts and classifies current
bytes against the last build snapshot, including uncommitted edits and deletions.
Client/build changes refuse with `run ./build.sh dev instead` before shutdown.
It compiles only the server and changed CLI sources while the old server runs,
verifies the new build/CLI binding, then uses authenticated protocol2 shutdown
and a pinned process-exit observation. It stages the new server/inventory,
refreshes provenance and launches the same instance, config, data and API port.
The client process/window, static frontend server and watcher stay alive.
A shutdown uncertainty refuses; it never force-kills or retries the swap.

The native window shows branded Reconnecting while detached. Its existing
10-second recovery verifies the current DEV inventory, reattaches to the new
epoch and navigates the same window to the interface. Recovery never replays
input or launches an old server while a server swap is in progress.
Server READY is authenticated server proof; confirm the actual native UI and
restored tab IDs separately. Frontend paint is not implied by public health.

## Select an isolated instance

Noncanonical worktrees require both settings and a private API port other than
7685/7686. Never inherit parent production selectors. The named operations
capture the report route, then clear parent URL/token/socket/data selectors and
export their selected instance and API port to children.

```bash
CODETERM_DEV_INSTANCE=my-lane CODETERM_DEV_API_PORT=7887 ./build.sh dev
CODETERM_DEV_INSTANCE=my-lane CODETERM_DEV_API_PORT=7887 ./build.sh dev-server
CODETERM_DEV_INSTANCE=my-lane CODETERM_DEV_API_PORT=7887 ./build.sh kill
```

PowerShell: set `$env:CODETERM_DEV_INSTANCE='my-lane'` and
`$env:CODETERM_DEV_API_PORT='7887'`, then use `./build.ps1 <operation>`.
Use the same selectors for the entire lifecycle. Optional frontend and diagnostics
ports use `CODETERM_DEV_VITE_PORT` and `CODETERM_DEV_CDP_PORT`.

Named Cargo artifacts live under
`src-tauri/target/dev-instances/<instance-key>/`. Data/config/logs and
`dev-build-provenance.json` live under `~/.codeterm-dev-<instance-key>/`.
Canonical defaults use `~/.codeterm-dev/`. Unix stages the native client under
`.domios-dev-runtime/bin` with paired `lib/codeterm` resources; Windows stages
resources beside the native executable. macOS raw-client DEV requires an
app-shaped signed pair and remains unsupported by this staging operation.

## Verify and diagnose

Compare `http://127.0.0.1:<selected-port>/api/health` build to the intended full
Git SHA before debugging. A new SHA requires a commit; provenance also hashes
uncommitted inputs and exact client, server, inventory, CLI and resource bytes.
Do not edit provenance or reset HEAD to make a stale build pass.

Use the READY/failure receipt from the Dev shell. Startup has bounded preparation,
bootstrap and attachment phases; no polling loop or fixed warmup proves readiness.
A busy build gateway is a refusal; do not retry it automatically.
Logs: `<selected-data-dir>/logs/codeterm.log` and the launch log named in the
receipt. Sanitized `DOMIOS_CLIENT_DIAGNOSTIC` boundaries identify install,
attach and native-enrollment failures without credentials.

For CLI/API benches, source `evals/_lib/lib.sh` and use `dc`, `dev_get`,
`dev_post` or `dev_run`; never raw `codeterm` inheriting production routing.
Use the [Browser procedure](../dev-testing/SKILL.md) or checked native diagnostics
for UI evidence. See the [build guide](../build/SKILL.md) for platform launch,
resource/provenance failures and Windows desktop/job diagnostics.

Use `build.sh kill` for exact selected-instance cleanup. It also stops that
instance's frontend/watchers and reaps its identified sidecars. Never use a
process-name kill. If identity is uncertain, preserve processes and report it.
A corrupt DEV config may be renamed after scoped shutdown; retain the backup.
