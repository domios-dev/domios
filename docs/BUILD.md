# CodeTerm — Build Guide

## Prerequisites

### Windows
- **Rust** 1.93+ (`rustup install stable`)
- **Node.js** 22+ with **pnpm** (`npm install -g pnpm`)
- **MSVC Build Tools** — Visual Studio 2022 Build Tools with:
  - `Microsoft.VisualStudio.Component.VC.Tools.x86.x64`
  - `Microsoft.VisualStudio.Component.Windows11SDK.22621`
- **WebView2** — comes with Windows 11, install separately on Windows 10

### macOS
- Xcode Command Line Tools
- Rust, Node.js, pnpm

### Linux
- `build-essential`, `libwebkit2gtk-4.1-dev`, `libssl-dev`, `libgtk-3-dev`
- Rust, Node.js, pnpm

## Quick Start

```bash
git clone <repo-url> codeterm && cd codeterm
pnpm install
./build.sh dev          # development (hot-reload, foreground)
./build.sh build        # production build
```

### Windows

```powershell
# Daily commands; run from the repository root.
.\build.ps1 dev           # foreground dev on 7686
.\build.ps1 check         # incremental compile check
.\build.ps1 worktree-check # canonical integration compile check
.\build.ps1 cli           # rebuild only the installed CLI
.\build.ps1 install-local # incremental production build + self-install
```

`build.ps1` is the Windows command hub; do not add a Makefile or hand-expand
its underlying Cargo, pnpm, gateway, or installer commands. `install-local`
already resumes the calling tab with a default continuation intent. Keep the
command short unless the continuation itself must be customized.

## Choose one operation

These flows are deliberately separate. Run every build, test, dev instance, and
executable gate from the warm canonical checkout under Orchestrator authority.
Isolated Worker lanes are source-only.

| Goal | Entry point | Touches production? | Read first |
|---|---|---:|---|
| Develop and inspect changes | `./build.sh dev` / `.\build.ps1 dev` | No; port 7686 only | `.agents/skills/dev-run/SKILL.md` |
| Build and test a local production installer | `./build.sh release-preflight`, then `./build.sh build` | Only the explicit self-install step | this section |
| Publish to all users | PR to `master`, then `release-X.Y.Z` tag | Yes; CI and public updater mirror | `docs/RELEASE.md` and `github-release` skill |

Never use a dev build as a local-install artifact, and never turn a local
installer build into a public release by uploading it manually.

## Local production build and self-install

Use this only when testing a production-mode build on this Windows machine. It
does not publish a release or alter GitHub assets.

1. Start from the intended, clean release branch in the canonical checkout.
2. After a version bump, run the idempotent preflight. It refreshes both Cargo
   lockfiles and verifies they match `package.json`:

   ```powershell
   .\build.ps1 release-preflight
   git add src-tauri/Cargo.lock cli/Cargo.lock
   git commit -m "release: sync Cargo lock versions"
   git status --short
   ```

3. For repeated local production testing, build and self-install through the
   persistent canonical cache:

   ```powershell
   .\build.ps1 install-local
   Invoke-RestMethod http://127.0.0.1:7685/api/health
   ```

   This route is Windows-only, requires the clean canonical checkout, reuses a
   persistent release target with sccache, rebuilds only changed Cargo units,
   recreates the NSIS bundle, verifies source provenance, and installs that
   exact artifact. When invoked inside CodeTerm, it saves the session and sends
   a continuation intent to the calling Tab after the new daemon is healthy.
   The default intent verifies the installed SHA and resumes the interrupted
   work. Override or disable it when needed:

   ```powershell
   .\build.ps1 install-local --continue-prompt "Verify the installed build, then resume release QA."
   .\build.ps1 install-local --continue-tab "<tab-id>" --continue-prompt "Continue the interrupted task."
   .\build.ps1 install-local --no-continue
   ```

   Cargo incremental remains disabled because sccache rejects incremental rustc
   invocations; persistent completed artifacts provide reuse.

   On Windows the CLI hands the installer helper to the system process broker
   before the running daemon is stopped. The helper must not inherit the
   calling Tab's PTY Job: detached stdio, `DETACHED_PROCESS`, `cmd start`, and
   `Start-Process` alone do not escape that Job and are not valid substitutes.
   The handoff invokes the freshly bundled CLI by its provenance-owned path;
   resolving `codeterm` from `PATH` may silently run the previously installed
   helper and is forbidden in this flow.
   Direct `update install --from-file` calls default to a short continuation
   when `CODETERM_TAB_ID` is present; use `--no-continue` only when interruption
   is intentional.
   A successful self-install is proven by `self-install.log` reaching
   `installer exited 0` and `launched`, followed by port 7685 health reporting
   the installed source SHA.

4. Use the clean-room route when validating a public-release-equivalent
   artifact:

   ```powershell
   .\build.ps1 build
   codeterm update install --from-file "<printed-installer-path>"
   ```

   `build` always creates a fresh nonincremental target under
   `%LOCALAPPDATA%\CodeTerm\build-gateway\targets\release\<job>\`. It remains
   intentionally slower and isolated. The updater helper stops only the
   current-user installed CodeTerm, runs NSIS, and restarts it.

### Local production troubleshooting

| Symptom | Cause | Recovery |
|---|---|---|
| `Release lock version mismatch` before build | Version manifests and one or both Cargo lockfiles differ | Run `release-preflight`, commit both lockfiles, then retry. |
| `release source is not clean` | A source or lockfile change has not been reviewed/committed | Inspect `git status --short`; commit the intentional change or stop. Never bypass provenance. |
| `source SHA/status mismatch` after a bundle was created | A build step changed tracked source inputs after the provenance snapshot | Do not install that artifact. Identify the mutating step, fix it, and rerun from a clean tree. |
| Self-install logs `starting` and the window never returns | A helper inherited the calling PTY Job and died when `taskkill` closed that Job | Rebuild the current CLI; it creates the helper through the out-of-job Windows process broker. Re-run `install-local`, inspect `%LOCALAPPDATA%\codeterm\self-install.log`, and verify port 7685 health. |
| CodeTerm restarts but the initiating tab does not resume | The continuation send failed or the Tab id changed before session persistence | Inspect `%LOCALAPPDATA%\codeterm\self-install.log`; the retained `%TEMP%\codeterm-continuation-*.txt` contains the undelivered intent. |
| Repeated local install recompiles the full release graph | The clean-room `build` route was used instead of the persistent local cache | Use `build.ps1 install-local`; reserve `build` for public-release-equivalent verification. |
| A focused Rust test spends its deadline linking app binaries | The test omitted `--lib`, so Cargo built unrelated binary test targets | Run the focused filter through `scripts/test-capped.sh ... --lib <filter>`; never raise the deadline. |
| A fresh lane's focused Rust test reports missing `../dist` or `bundled-cli/*` | Ignored Tauri resource directories have not been staged in that worktree | Run `node scripts/dev-resource-preflight.mjs` and `node scripts/prepare-cli-stub.js`, then rerun the same capped focused test. Do not substitute a frontend build or raise the cap. |
| `sccache: incremental compilation is prohibited` | `CARGO_INCREMENTAL=1` was combined with the shared sccache wrapper | Keep `CARGO_INCREMENTAL=0`; reuse the persistent completed target and sccache entries instead. |
| Release-signing negative test passes despite a missing fixture key | The shell inherited `TAURI_SIGNING_PRIVATE_KEY` from the canonical `.env` | Run the repository check; its missing-key fixture explicitly removes the inherited key before asserting failure. |
| Push unexpectedly starts compilation or formatting checks | A stale local hook predates the non-blocking push policy | Run any `build.ps1` command to restore `.githooks`, then retry; verification belongs to explicit gates and CI. |
| `Build gateway job id already exists` immediately from a fresh command | The calling CodeTerm tab inherited a completed gateway job id from an earlier build | Update the build scripts; production gateway entry now always creates a fresh job id and ignores inherited internal ids. |
| `pnpm build` fails only in focused TSX test files during `install-local` | A test mock violates the generic bridge signature or imports an unused JSX runtime symbol | Run `pnpm build`, fix the test type/import at the source, then retry `install-local` from the same clean canonical checkout. |
| Installed health on port 7685 reports a dev identifier or Vite/CDP ports | The detached installer helper inherited `CODETERM_*` and Cargo build variables from the calling tab | Use the current installer helper, which scrubs CodeTerm and build variables before launching the installed daemon; reinstall and verify `/api/health`. |
| NSIS reports `Error opening file for writing` for `conpty\<arch>\OpenConsole.exe` | A bundled ConPTY host from the installed CodeTerm instance survived PTY teardown and still holds its executable | Current builds reap path-matched owned hosts before NSIS. On an older build, verify the process image is under `%LOCALAPPDATA%\CodeTerm\conpty\`, end only that `OpenConsole.exe`, then click **Retry**; never end `conhost.exe` or an `OpenConsole.exe` from another path. |
| Installed CodeTerm exits during startup with a missing `tab_id`/`tabId` field or `delayed_send init` panic | A pane-era or malformed persisted record reached an all-or-nothing startup deserializer | Rebuild and install the current revision, then restart. Inspect `[plan.store]`, `[delayed_send]`, `[remote-workers]`, `[graph-workers]`, and `[session]` diagnostics for the isolated record; do not delete `plans.db` or an entire state JSON file. |

When a new failure is found in this flow, fix the owning script or preflight and
add a short symptom/cause/recovery row here before closing the work.

Do not use `dev --wait` for an interactive Windows session. It is reserved for
automation that needs a detached health gate and does not prove a visible window.

### Visible Windows dev session

For interactive verification, do not use `dev --wait`. It is the detached
readiness path for scripts and CI. Use one visible CodeTerm Tab as the sole
foreground owner:

```powershell
# Verify no previous dev listener/process remains, then open one visible tab.
codeterm tab new --cmd "Set-Location '<checkout>'; .\build.ps1 dev"
```

Keep that Tab open and require all of the following before reporting ready:

- port 7686 is owned by the checkout's `target\debug\codeterm.exe`;
- `/api/health` reports `status=ok`, the exact checkout `build_sha`, API 7686,
  Vite 1420, CDP 9223, and identifier `com.codeterm.dev`;
- the CodeTerm process has a responsive, nonzero `MainWindowHandle`;
- exactly one Cargo process remains, the foreground `tauri dev` watcher owned
  by the visible Tab. Any second Cargo/build owner must be stopped and traced.

The canonical checkout owns shared dev, broad gates, and release checks. This is
a team operating rule, not a branch-name restriction in the build script; the
machine-wide gateway only serializes heavy app work.

Gateway-backed commands fail fast when another job owns the machine slot, so
an agent turn never blocks silently behind a long-lived dev process. Add
`--wait-build` only when synchronous queueing is intentional.

Use `./build.sh help` as the command index. `test-one <app|plan|pty|mem|plugin|cli> <filter>`
hides manifest/package details for focused capped tests; `verify` runs the
standard check, unit-test, and source-size integration gate.

`./build.sh cli` is intentionally independent from the app/dev build gateway:
Cargo serializes its separate `cli/target` itself. The command also regenerates
`docs/cli-manifest.json` and the generated block in `docs/CLI.md`, then installs
the release binary only into the active development data directory. Dev builds
never overwrite `~/.codeterm/bin`: production CLI updates come from the bundled
production app during startup, in-app update, or `install-local`.

### Development troubleshooting

| Symptom | Cause | Recovery |
|---|---|---|
| A build exits with `Timed out acquiring build gateway state mutex` | Another live or stale gateway owner held the state mutex through the acquisition deadline. The failed invocation now terminates only its own Cargo/rustc descendants before returning. | Run `./build.sh gateway-status` to inspect the active and queued job IDs. If a job is stale, run `./build.sh gateway-cancel <job-id>`, then rerun the original command. |
| `build.ps1 check` exits 75 because the visible dev owns the gateway | The canonical incremental target is intentionally single-owner; waiting behind a foreground dev process cannot complete. | Stop and make an explicit canonical integration decision. Do not move the command into a Worker lane, add `--wait-build`, or hand-compose `CARGO_TARGET_DIR`. |
| A Windows dev build fails while writing `codeterm_lib.lib` with `os error 112` or a localized “not enough disk space” message | The canonical `src-tauri/target` exhausted the checkout drive while Cargo was replacing the debug archive. | Verify the failure owns the canonical target and no dev build is running, then run `.\build.ps1 clean`. Confirm free space before restarting `.\build.ps1 dev`; do not delete another worktree's target or touch production. |
| A Windows native dependency fails with `LNK1106` and `dumpbin` shows a cached `.o` ending before its COFF string-table length | `sccache` retained a truncated C/C++ object; a clean Cargo target can restore the same corrupt cache entry. | In PowerShell set `$env:SCCACHE_RECACHE='1'`, rerun the failing `.\build.ps1` operation once, then remove the environment variable. Verify the rebuilt object with `dumpbin` before retrying normally. |
| Dev compiles and opens but port 7686 never listens, with a trust-boundary policy error in `~/.codeterm-dev/logs/codeterm.log` | The retained dev config requests a non-loopback host without a bearer token, or enables CORS without an allowlist. | Keep dev local by setting `api.host = "127.0.0.1"` and `api.cors = false` in `~/.codeterm-dev/config.toml`, then restart the visible dev tab. For an intentional network-facing instance, use a token and enable Mesh. |
| `test-one plan` fails to compile old `Task` fixtures after an orchestration schema change | The app gate compiled the plan crate as a dependency but did not compile its own unit-test fixtures. | Update every direct `Task` fixture and changed helper signature, then rerun the exact `test-one plan` filter. Do not remove the plan gate or move it to an app filter. |

## Related documents

Tooling, outputs, platform notes, and repository layout: `codeterm docs BUILD-REFERENCE`. Test workflows: `codeterm docs TESTING`.

<!-- revision: 51 -->
