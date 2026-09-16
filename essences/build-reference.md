# Build reference

Dev/prod isolation, hooks, signing, outputs, icons, platform notes, and project layout.

## Dev/Prod Isolation

Dev builds (`npx tauri dev`) run with `tauri.dev.conf.json` overrides:
- **Identifier:** `com.codeterm.dev` (vs `com.codeterm.app` in prod)
- **API port:** 7686 (vs 7685 in prod)
- **Data directory:** `~/.codeterm-dev/` (vs `~/.codeterm/`)
- **Socket prefix:** `codeterm-dev` (vs `codeterm`)
- **Single-instance:** Disabled (dev and prod can run side by side)

### macOS dev launch contract

The supported dev GUI path is the raw Tauri debug executable, not a `.app`
bundle. `tauri.dev.conf.json` has no bundle target, and `build.sh` resolves
`dev_app_path` to `src-tauri/target/debug/codeterm`; the prebuilt launcher
starts that executable from the repository checkout. The production config
does enable the `.app` bundle target, so its absence from a dev target is
intentional rather than a missing production package.

Use the repository launcher when reproducing or verifying a dev daemon; it
selects the isolated dev ports and performs the health/build-identity check:

```bash
./build.sh dev-prebuilt --wait
```

If a fresh dev artifact is required, use `./build.sh dev --wait` first. Do not
launch `src-tauri/target/debug/codeterm` through macOS `open`; that is outside
the supported dev lifecycle and does not carry the launcher’s instance
environment. Production `.app` verification belongs to the release bundle
path (`pnpm tauri build`), not this raw-executable dev oracle.

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

Test layers, commands and the runbook: `essences/testing.md`.

### Test troubleshooting

| Symptom | Cause | Recovery |
|---|---|---|
| `Build gateway busy` returns immediately | Another build or the visible dev watcher owns the machine slot; the requested command did not start. | Read the reported job, owner, and command. Run later, or add `--wait-build` only when blocking the caller is intentional. |
| `qa-prep` fails at stage `dist:node-modules-link` with `Parameter format not correct - <worktree>\node_modules` | On Windows, MSYS rewrites the bare `mklink /J` switch into the drive path `J:/` before `cmd` sees it, so `mklink` never receives the junction switch and rejects the arguments | Rerun `./build.sh qa-prep` on a build that passes the switch as `//J` (escaped from MSYS path conversion); the junction to the canonical checkout's `node_modules` is then created without elevation |
| `pnpm install --frozen-lockfile` fails with `ENOENT` for a package inside `node_modules/.pnpm` | The local pnpm virtual store contains a partial package directory, so the lockfile is valid but linking cannot read the package manifest | Run `pnpm install --frozen-lockfile --force`, verify the missing binary or package is present, then rerun the original test command |


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

Public releases: the map is `essences/release.md` and the executable procedure is the
`github-release` skill (`.agents/skills/github-release/SKILL.md`). Local builds with no
release use `./build.sh build`, above.

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
running. Do not delete the shared build cache.

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

