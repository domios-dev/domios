# CodeTerm self-update via tauri-plugin-updater

**Branch:** `feat/auto-update`
**Status:** queued — pick up after `feat/prod-resilience` is merged.
**Source:** Andrey 2026-04-25, expanded into a brief here.

## Goal

CodeTerm should be able to update itself when a new version is published on GitHub. The release contains platform binaries (`.msi`/`.exe` for Windows, `.dmg`/`.app.tar.gz` for macOS). After applying, CodeTerm restarts cleanly without losing sessions, mem state, or active Claude agents — the user sees a brief blip, not a context wipe.

## Non-goals (v1)

- Differential / binary patches (always full asset).
- Rollback UX beyond manually downloading the previous release.
- Linux packaging (revisit when there's demand).
- Forcing updates — always opt-in, never auto-applied without confirmation.

## Architecture

### Plugin

`tauri-plugin-updater` (official Tauri 2.x). Provides:
- Manifest fetcher
- ed25519 signature verification (refuses unsigned/wrong-key)
- Cross-platform install dispatcher (atomic binary swap on Windows; rsync into `.app` on macOS)
- `install_and_restart()` API

### Manifest

`latest.json` published as a release asset:

```json
{
  "version": "1.0.2",
  "notes": "Markdown-formatted release notes",
  "pub_date": "2026-04-25T12:34:56Z",
  "platforms": {
    "windows-x86_64":  { "url": "https://github.com/.../codeterm-1.0.2-x64.msi.zip",        "signature": "<base64 ed25519>" },
    "darwin-aarch64":  { "url": "https://github.com/.../codeterm-1.0.2-aarch64.app.tar.gz", "signature": "<base64 ed25519>" },
    "darwin-x86_64":   { "url": "https://github.com/.../codeterm-1.0.2-x64.app.tar.gz",     "signature": "<base64 ed25519>" }
  }
}
```

### Channels

- `stable` → `latest.json` on the latest non-prerelease tag
- `rc`     → `latest-rc.json` on tags matching `v*-rc*`
- `nightly`→ `latest-nightly.json` on `develop` builds (CI nightly job)

User-selectable via `codeterm config set updater.channel <stable|rc|nightly>`.

### Signing

1. Generate ed25519 keypair once: `cargo tauri signer generate -w ~/.tauri/codeterm.key`.
2. Embed public key in `tauri.conf.json` (`plugins.updater.pubkey`).
3. Add private key + password to GitHub Secrets as `TAURI_PRIVATE_KEY` and `TAURI_KEY_PASSWORD`.
4. CI job calls `tauri-action` with these secrets — produces signed assets and the `latest.json` automatically.

## CI

New workflow `.github/workflows/release.yml`:

- Trigger: `push` of tag `v*`.
- Matrix: `windows-latest` (x86_64), `macos-13` (x86_64), `macos-14` (aarch64).
- Steps: checkout, install Rust + Node, `bun install`, `cargo tauri build --target $TARGET`, sign with `tauri-action`, upload artifacts to release, generate platform-merged `latest.json`, push to release as asset.

macOS notarization: `xcrun notarytool submit ... --wait` against the `.app.tar.gz` before upload. Apple Developer ID cert in `MACOS_CERTIFICATE` secret.

Windows: per-user MSI (no admin) — set `WixUI_InstallScope=perUser` in the bundle config.

## CLI surface

```
codeterm update --check                               # query manifest, print diff or "up to date"
codeterm update --apply                               # download + verify + apply + restart
codeterm update --channel <stable|rc|nightly>         # switch channel
codeterm config get updater                           # current channel + last check time + manifest URL
```

`--check` is the only one safe to wire into a 24h cron — never auto-apply.

## UI

- Footer badge: green dot when up to date, blue "Update available" pill when newer version on the configured channel.
- Click the pill: modal with version diff, release notes, "Update now" button.
- "Update now" → progress (downloading X/Y MB), then graceful shutdown sequence, then automatic relaunch.

## Graceful shutdown sequence

This is the make-or-break part — sessions and active Claude agents must survive.

1. UI / CLI confirms apply.
2. For each pane with a Claude/Codex agent: send the provider-specific exit command (Claude: `/exit`; Codex: `:quit`). This fires whatever `Stop` / `PreCompact` hooks the providers have, so transcripts and mem flushes complete.
3. Wait up to **10s** per pane. Track via PID exit watcher.
4. Stragglers: SIGTERM, then 5s grace, then SIGKILL.
5. Backend flushes `mem.db` (WAL checkpoint via `PRAGMA wal_checkpoint(TRUNCATE);`), closes all sqlite connections.
6. Backend invokes `tauri::updater::install_and_restart()`. The updater extracts new binaries, replaces them atomically:
   - **Windows**: writes new exe to `%TEMP%`, then `MoveFileEx` with `MOVEFILE_REPLACE_EXISTING | MOVEFILE_DELAY_UNTIL_REBOOT` fallback if locked; spawns new process before exiting current.
   - **macOS**: extracts `.app` next to old, swaps via `rename(2)` — POSIX-atomic.
7. New process starts, reads `~/.codeterm/state.json`, restores workspaces and panes.
8. For each restored pane, runs the saved provider command with `--resume <session_id>` — provider picks up its prior conversation.
9. User sees the same panes, same conversations, just a few-second blackout.

Mechanism #8 already exists for the `/exit` flow today (referenced in `AGENT.md`: "If you need to restart, use /exit — CodeTerm auto-resumes you"). Extending it to "after self-update" is reusing the same code path.

## Code map (where things land)

```
codeterm/
├── src-tauri/
│   ├── Cargo.toml                              + tauri-plugin-updater dep
│   ├── tauri.conf.json                         + plugins.updater config
│   └── src/
│       ├── lib.rs                              register updater plugin
│       ├── updater/
│       │   ├── mod.rs                          high-level facade
│       │   ├── manifest.rs                     fetch + parse + version compare
│       │   ├── channel.rs                      stable/rc/nightly resolution
│       │   ├── shutdown.rs                     graceful pane teardown
│       │   └── apply.rs                        invoke install_and_restart, error mapping
│       └── api/
│           └── updater_routes.rs               REST endpoints for the UI badge + modal
├── src/
│   └── components/
│       ├── UpdateBadge.tsx                     footer pill
│       └── UpdateModal.tsx                     confirm + progress + release notes
├── cli/
│   └── src/commands/update.rs                  `codeterm update --check|--apply|--channel`
└── .github/workflows/release.yml               new
```

## Open questions

- **Per-user vs system-wide install on Windows**: per-user is friendlier (no admin), but blocks shared-machine use cases. Default per-user; document `--all-users` flag for the MSI.
- **Auto-check cadence**: poll on app start + once per 24h while running. Configurable, off by default after startup poll.
- **Partial rollouts**: skip for v1.
- **Offline**: fail gracefully — never block startup on a manifest fetch.
- **Telemetry**: do we want to know update success/failure rates? Probably yes once telemetry exists; punt for now.

## Testing (mandatory, before any v1 ship)

You cannot ship a self-update feature without proving it works without bricking real installs. The dev instance (`~/.codeterm-dev/` on this machine) is the staging ground.

### Test surfaces

1. **Local manifest override**. `codeterm update --check --manifest-url file:///tmp/test-manifest.json` lets us point the updater at a hand-rolled JSON without a GitHub round-trip. Same for `--apply --manifest-url ...`. Required for everything below.

2. **Local asset server**. `cd /tmp/test-assets && python -m http.server 8080` to host pre-built `.msi.zip` / `.app.tar.gz` locally. Manifest URLs reference `http://127.0.0.1:8080/...`. No internet, no GitHub rate limits, no waiting on CI.

3. **Version inversion trick**. In dev builds, set `version` in `tauri.conf.json` to `0.0.1`. Build that, install as the "current" version. Then build another build with version `0.0.2` and serve it via the local manifest. Run `update --apply` and watch the dev instance upgrade itself. End-to-end verifies binary swap, signature check, restart, session resume.

4. **Signature negative-path**. Flip a byte in the test asset, regenerate manifest with the OLD signature, point updater at it. Update must abort, log the verification failure, leave the old binary in place. Add this as a CI integration test.

5. **Pane survival**. Before triggering update, spawn 2–3 Claude/Codex agents in dev panes mid-conversation. Apply update. Verify all panes restored AND each agent session resumed with prior context (check transcript continuity, not just "pane exists"). Add `tests/e2e/update-resume.spec.ts` for this.

6. **Failure modes**:
   - Network drops mid-download → updater retries 3× with backoff, then aborts cleanly.
   - Disk full during extract → abort, leave old binary, surface error.
   - Crash on first launch of new version → user can manually downgrade by reinstalling old asset (rollback is out of scope for v1, document the workaround).
   - Two concurrent `--apply` invocations → second one detects lock and refuses.

### Test instance setup (dev codeterm)

```bash
# In ~/.codeterm-dev/, isolated from prod
codeterm-dev workspace create "Update Test" --cwd /tmp/codeterm-update-rig
# Build two versions side by side
cd /c/Developer/codeterm
git checkout feat/auto-update
sed -i 's/"version": ".*"/"version": "0.0.1"/' src-tauri/tauri.conf.json
bun run tauri build --target x86_64-pc-windows-msvc -- --features dev-updater-allowlocal
cp src-tauri/target/release/bundle/msi/CodeTerm_0.0.1_x64_en-US.msi /tmp/test-assets/codeterm-0.0.1.msi
sed -i 's/"version": "0.0.1"/"version": "0.0.2"/' src-tauri/tauri.conf.json
bun run tauri build --target x86_64-pc-windows-msvc -- --features dev-updater-allowlocal
cp src-tauri/target/release/bundle/msi/CodeTerm_0.0.2_x64_en-US.msi /tmp/test-assets/codeterm-0.0.2.msi
# Sign both with the dev keypair (separate from prod keypair!), generate test-manifest.json
# Serve, install 0.0.1, then run codeterm update --apply --manifest-url http://127.0.0.1:8080/manifest.json
```

`dev-updater-allowlocal` is a Cargo feature flag that whitelists `file://` and `http://127.0.0.1:*` manifest URLs. Stripped from release builds — production never accepts unsigned local manifests.

### CI test job

Add `update-smoke` job to `.github/workflows/release.yml` (gated on PR labels for cost):

1. Matrix-build a `0.0.0-test` and a `0.0.1-test` version of codeterm with `dev-updater-allowlocal`.
2. Spin a sandbox VM (Windows + macOS), install the older version, launch headlessly with no panes (`--no-restore`).
3. Run `codeterm update --apply --manifest-url file:///tmp/manifest.json` against a fake manifest pointing at the newer asset.
4. After the relaunch, assert version reported by `codeterm --version` equals `0.0.1-test`.
5. Tear down. Pass the job only if the binary swapped and the new process answered.

This is the regression net — anyone breaking the apply pipeline in a future PR fails the smoke job before merge.

### Manual sign-off checklist before promoting from `feat/auto-update` to `develop`

- [ ] Local manifest override works on Windows.
- [ ] Local manifest override works on macOS Apple Silicon.
- [ ] Local manifest override works on macOS Intel.
- [ ] Tampered asset is rejected (negative test).
- [ ] Live Claude pane survives update with conversation intact.
- [ ] mem.db is intact (run a `mem search` post-update; row count matches pre-update).
- [ ] No lingering processes from old binary (`tasklist | grep codeterm` shows only the new PID on Windows; equivalent on macOS).
- [ ] `--check` is reliable on flaky network (kill wifi mid-call → graceful timeout, not hang).

## Acceptance

- Releasing a new tag publishes signed binaries + `latest.json` to GitHub Releases without manual intervention.
- Users see the "Update available" pill within 5 minutes of release.
- Clicking "Update now" successfully replaces the binary and relaunches with all panes / Claude sessions resumed in <30s.
- A user opted into `nightly` channel sees develop-branch builds; `stable` users do not.
- `codeterm update --check` exits non-zero when on the latest version (so cron can branch on it), exit zero with diff when newer is available — confirm exit-code direction with stakeholder before wiring.
- Tampering with the asset (flip a byte) → verification fails, install aborts, old binary preserved, error surfaced clearly.

## Cross-references

- `feat/voice-transcription-plugin` — independent, can ship before or after.
- `feat/prod-resilience` — must merge before this, since the graceful-shutdown logic shares code paths with the daemon supervisor and pane lifecycle improvements.
- Project mem note: `note_feature_codeterm_self_update_via_tauri_plugin_updater`.
