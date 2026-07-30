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
.\build.ps1 cli           # rebuild only the installed CLI
.\build.ps1 install-local # incremental production build + self-install
```

`build.ps1` is the Windows command hub; do not add a Makefile or hand-expand
its underlying Cargo, pnpm, gateway, or installer commands. `install-local`
already resumes the calling tab with a default continuation intent. Keep the
command short unless the continuation itself must be customized.

## Choose one operation

These flows are deliberately separate. Run all shared builds and gates from the
warm canonical checkout; lanes may only run focused checks or named isolated dev
instances.

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
| Self-install returns but the window does not return | The detached helper owns restart | Inspect `%LOCALAPPDATA%\codeterm\self-install.log`, then query port 7685 health. |
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

## Dev/Prod Isolation

Dev builds (`npx tauri dev`) run with `tauri.dev.conf.json` overrides:
- **Identifier:** `com.codeterm.dev` (vs `com.codeterm.app` in prod)
- **API port:** 7686 (vs 7685 in prod)
- **Data directory:** `~/.codeterm-dev/` (vs `~/.codeterm/`)
- **Socket prefix:** `codeterm-dev` (vs `codeterm`)
- **Single-instance:** Disabled (dev and prod can run side by side)

## Git Hooks

Custom git hooks live in `.githooks/` and are automatically wired via `git config core.hooksPath` by every `./build.sh` invocation. The pre-push hook is intentionally a no-op: pushing never compiles, formats, lints, or prepares resources. Run focused checks before handoff and broad gates in the canonical checkout; CI remains the remote enforcement boundary.

Focused Rust tests use the same Windows rule. `scripts/test-capped.sh` keeps the
manifest's warm target normally and selects checkout-local `target-check` with
`sccache` only while dev owns the app target. It rejects an inherited internal
build-gateway target because that cache can contain absolute metadata from
another checkout.

`build.ps1 dev` is also a true restart command: it stops only port 7686 before
joining the app build gateway. This ordering prevents the old dev monitor from
waiting on itself while retaining ownership of the Windows build artifacts.

## macOS Code Signing

**Release builds MUST be signed.** Unsigned DMGs trigger Gatekeeper ("unidentified developer").

```bash
# Signing identity (required for release builds)
APPLE_SIGNING_IDENTITY="Developer ID Application: <YOUR NAME> (<TEAM_ID>)"

# Build signed DMG
APPLE_SIGNING_IDENTITY="Developer ID Application: <YOUR NAME> (<TEAM_ID>)" pnpm tauri build
APPLE_SIGNING_IDENTITY="Developer ID Application: <YOUR NAME> (<TEAM_ID>)" pnpm tauri build --target x86_64-apple-darwin
```

Add to `.env` in project root (gitignored) so `build.sh` picks it up automatically:
```
UPDATER_TOKEN=<github PAT>
APPLE_SIGNING_IDENTITY=Developer ID Application: <YOUR NAME> (<TEAM_ID>)
```

For notarization (optional, removes "downloaded from internet" warning):
```bash
APPLE_ID=<apple-id-email>
APPLE_PASSWORD=<app-specific-password>
APPLE_TEAM_ID=<TEAM_ID>
```

## Build Output

After `./build.sh build`:

| File | Location |
|------|----------|
| **NSIS installer** | printed build-gateway release target; see Local production build and self-install |
| **MSI installer** | `src-tauri/target/release/bundle/msi/CodeTerm_X.Y.Z_x64_en-US.msi` |
| **Portable exe** | `src-tauri/target/release/codeterm.exe` |
| **CLI binary** | bundled in the production app; installed to `~/.codeterm/bin/` by production startup/update |

## Running Tests

```bash
./build.sh test                                            # Rust unit tests (~307)
npx tsx src/components/Terminal/chatEnrichment.test.ts      # TS chat enrichment tests
./build.sh e2e                                             # E2E (needs running dev instance)
```

### Test troubleshooting

| Symptom | Cause | Recovery |
|---|---|---|
| `qa-prep` fails at stage `dist` with `ln: .../node_modules: cannot overwrite directory`, or a later `vite build` fails while `node_modules` is only a few hundred KB | On Windows, MSYS `ln -sf` copies instead of symlinking and aborts midway, leaving a partial `node_modules` stub in the worktree that later runs mistake for a real install | Delete the stub (`rm -rf node_modules`) and rerun `./build.sh qa-prep` — it now creates a junction (`mklink /J`) to the canonical checkout's `node_modules` on Windows |


If `.\build.ps1 test` stops before the Rust summary with `os error 232` or "the
pipe is being closed", the build command lost its stdout reader; this is a
gateway logging failure, not a test verdict. The gateway now owns the `tee`
reader, closes its writer descriptors after the final exit-status line, and
waits for the reader before returning to PowerShell. A broken path reports exit
74 and names the log separately from the command's exit. Run
`./build.sh gateway-logging-check` to verify final-line delivery and the
distinct logging-failure path. For diagnosis only,
`CT_BUILD_GATEWAY_NO_TEE=1` bypasses log duplication; unset it for normal
builds. `CT_BUILD_GATEWAY_FAIL_LOG_OPEN=1` is a test-only fault injector at the
log-open boundary; it must report the logging failure and exit 74 before cargo
starts.

## Release Process

Releases are cut by pushing a single `release-X.Y.Z` tag on `master`, which fires three
self-hosted CI runners (macOS + Linux arm64 on the Mac, Windows on the ROG, Linux x86_64 on the
ROG's WSL2) in parallel — not the local/mesh-coordinated flow this section used to describe.

Full runbook (preflight, exact trigger-tag commands, artifact matrix per platform, public-mirror
verification, safe reruns without retagging, troubleshooting, cleanup): **`docs/RELEASE.md`**.
Executable step-by-step procedure with every command and secret: `.agents/skills/github-release/SKILL.md`.
Build-farm/runner internals: `.codeterm/build-machine/README.md`.

Local-only builds (no release) still use `./build.sh build` as documented above under Build Output.

## Icons

Icon source files: `src-tauri/icons/`

| File | Size | Usage |
|------|------|-------|
| `icon.png` | 512x512 | Window icon, installer |
| `256x256.png` | 256x256 | High-DPI displays |
| `32x32.png` | 32x32 | System tray (Windows/Linux) |
| `tray-template.png` | — | macOS tray (template image, auto light/dark) |
| `icon.ico` | 256x256 | Windows executable |

**Bundle icons** (`tauri.conf.json` → `bundle.icon`) are embedded into the built installer/exe.

**Runtime icons** are set programmatically in `src/lib.rs` setup — needed for dev mode and custom title bar:
- Window icon: decoded from `icon.png` via `png` crate → `window.set_icon()`
- Tray icon: decoded from `tray-template.png` (macOS, white on transparent, auto-adapts to light/dark) or `32x32.png` (Windows/Linux) → `TrayIconBuilder::icon()`

### Regenerating icons

1. Edit SVG source or create new design
2. Render to PNG at multiple sizes using `sharp` (Node.js) or ImageMagick
3. Replace files in `src-tauri/icons/`
4. Rebuild — icons are embedded at compile time via `include_bytes!()`

### Windows icon cache

If the icon doesn't update after rebuild, clear Windows icon cache:
```bash
rm -f "$LOCALAPPDATA/IconCache.db"
rm -rf "$LOCALAPPDATA/Microsoft/Windows/Explorer/iconcache*"
rm -rf "$LOCALAPPDATA/Microsoft/Windows/Explorer/thumbcache*"
taskkill /F /IM explorer.exe && start explorer.exe
```

## Windows MSVC Notes

If build fails with `link.exe` errors, ensure MSVC is properly configured. The project includes `.cargo/config.toml` with MSVC paths. You may need to update paths if your MSVC version differs.

If a focused test fails while `link.exe` or `mt.exe` reads
`windows-app-manifest.xml` from a deleted worktree, the shell inherited
CodeTerm's private local-install `CARGO_TARGET_DIR`. Run the test through
`scripts/test-capped.sh`; it removes that private override and uses this
checkout's warm manifest target, or isolated `target-check` while dev is
running. Do not delete the shared build cache or raise the timeout.

If a focused Rust test fails with `PermissionDenied (os error 5)` while replacing
a Tauri build artifact, another dev/build owner still holds the canonical target.
Verify listeners first, stop only the dev instance on 7686 (or its explicitly
named non-production port), and rerun `scripts/test-capped.sh`. Never stop 7685.

Check your MSVC installation:
```powershell
& "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe" -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
```

## Project Structure

```
codeterm/
├── src-tauri/          # Rust backend (Tauri v2)
│   ├── src/
│   │   ├── lib.rs      # App entry, state setup
│   │   ├── commands.rs  # PTY Tauri commands
│   │   ├── pty/         # PTY manager (portable-pty)
│   │   ├── multiplexer/ # Workspace/Tab/Tab state
│   │   ├── agent/       # Agent detection, git info
│   │   ├── socket/      # JSON-RPC socket server
│   │   ├── api/         # REST API (Axum)
│   │   ├── notification/# Notification store
│   │   ├── config/      # TOML config + themes
│   │   ├── prompts.rs   # Agent prompt templates
│   │   └── session/     # Session persistence
│   └── Cargo.toml
├── src/                # React frontend
│   ├── App.tsx
│   ├── components/
│   │   ├── Terminal/    # xterm.js, splits
│   │   ├── Sidebar/     # Workspace/tab list
│   │   ├── Titlebar/    # Custom titlebar
│   │   ├── Notifications/
│   │   └── Settings/
│   ├── store/           # React context state
│   └── hooks/
├── cli/                # CLI binary
├── skills/             # Claude Code skills
├── tests/              # E2E tests
└── docs/               # Documentation
```

REST liveness troubleshooting: a bound socket with an unanswered health request
is `workers_blocked`, not a listener exit. Capture `[chat_start.bounded]`,
`[LOCK-HOLD]`, and `[SLOW-IPC]` lines, then run
`bash evals/api-liveness-mesh-load/run.sh` against dev 7686. The representative
cause is oversized session parsing on a REST worker; keep it bounded and on
blocking workers. A refused socket is instead listener exit or bind ownership
failure and belongs to the generation supervisor. If a Windows manual restart
reports bind failure immediately after a healthy generation, verify the health
proof consumes its body and closes its connection before publishing readiness.
If the macOS full Rust gate reports a REST lifecycle deadline while focused
listener tests pass, inspect whether the real `/api/health` proof exhausted its
bounded failure deadline under runner contention. Keep the proof event-driven;
do not replace it with a settle delay or treat elapsed time as readiness.
On Windows, an intermittent `os error 10048` during repeated manual restart
means either the completed server future still owns its listener or the socket
cannot reclaim its own address. Drop the owned future before rebinding and keep
the listener's reuse-address policy consistent across generations.
If `build.ps1 check` reports E0603 for a helper under `commands::<module>`,
call its `commands` facade re-export instead of making the implementation module
public. The command modules are intentionally private ownership boundaries.

<!-- revision: 34 -->
