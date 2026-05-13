---
title: Self-install codeterm from a local NSIS bundle (Windows)
area: deploy
tags: [release, install, windows, nsis, self-update]
owner: blackstar
scope: shared
source: git
---

# Self-install codeterm from a local NSIS bundle (Windows)

Replace the running codeterm daemon with a freshly-built `.exe` without going through the public updater. Useful for testing local builds, air-gapped boxes, or any flow where the public mirror isn't appropriate yet.

## When to use

- You just ran `./build.sh build` on Windows and want the new daemon running without waiting for a public release.
- The public-updater path is broken (Tauri-plugin-updater fetch failing, anti-virus blocking the manifest, etc) and you need an offline option.
- You're iterating on `--from-file` itself or any post-restart auto-resume logic.

NOT for the public-mirror release flow — that goes through the regular `gh release` upload + `latest.json` publish per the `github-release` skill. This skill is for local dev/test only.

## How it works

The CLI subcommand `codeterm update install --from-file <PATH>` writes a temp `.bat` helper, spawns it detached (so it survives the calling daemon's death), and exits. The helper:

1. Sleeps ~2s (lets the calling CLI return cleanly).
2. `taskkill /F /IM codeterm.exe codeterm-embed.exe`.
3. Runs the NSIS installer with `/S` (silent, no UAC since `installMode=currentUser`).
4. Launches the freshly installed `codeterm.exe`.
5. If `--continue-prompt TEXT` was passed, polls `/api/health` until 200, then `codeterm send-text --pane <id> --from-file <temp>` so the calling agent's session resumes with that prompt.
6. Logs every step to `%LocalAppData%\codeterm\self-install.log`.

The same flow is also exposed as a standalone PowerShell script at `./scripts/self-install-local.ps1` for cases where the daemon's CLI isn't reachable (e.g. completely fresh machine).

## Steps — preferred path (CLI)

```bash
codeterm update install --from-file 'C:\Developer\codeterm\src-tauri\target\release\bundle\nsis\CodeTerm_X.Y.Z_x64-setup.exe'
```

With session continuation:

```bash
codeterm update install \
  --from-file '...\CodeTerm_X.Y.Z_x64-setup.exe' \
  --continue-prompt "после рестарта проверь скорость табов и отчитайся"
```

`--continue-pane` defaults to `$CODETERM_PANE_ID`. Pass it explicitly only when invoking from outside a managed pane.

## Steps — fallback (PowerShell)

When the CLI is itself broken or version-mismatched:

```powershell
powershell -ExecutionPolicy Bypass `
  -File "$env:USERPROFILE\.codeterm\skills-shared\self-install-local\scripts\self-install-local.ps1" `
  -Installer "C:\Developer\codeterm\src-tauri\target\release\bundle\nsis\CodeTerm_X.Y.Z_x64-setup.exe" `
  -Resume
```

## Verifying the install

After ~5–8 seconds the new daemon should be up. Check:

```bash
curl -s http://127.0.0.1:7685/api/health
# → {"version":"X.Y.Z","build":"<short-sha>",...}

# Confirm the installed binary matches what you built:
sha256sum "$env:LocalAppData\codeterm\codeterm.exe" `
          "C:\Developer\codeterm\src-tauri\target\release\codeterm.exe"
# → both hashes should match
```

If the hashes differ, your `--from-file` path resolved to an older bundle. Re-run with the freshly built path. NSIS doesn't care about version drift between consecutive installs — it just unpacks whatever `.exe` payload is inside.

## Gotchas

- **Auto-resume of agent panes is best-effort.** The codeterm daemon restores `claude_session_id` from `~/.codeterm/sessions/last.json` and the frontend's `spawn_pty` resumes via `claude --resume <id>`. If `last.json` was stale (NSIS killed the daemon abruptly between auto-saves), the resumed session may be slightly behind the actual conversation. That's a known limitation, not specific to this skill.
- **Don't pass `-Verb RunAs` to the installer.** `installMode=currentUser` means the installer doesn't need elevation; adding `RunAs` triggers an interactive UAC dialog that won't be clicked on a headless machine and the install will hang.
- **Helper survives the daemon, not your terminal.** If you invoke `codeterm update install --from-file ...` from a CodeTerm-managed pane, the calling pane will die with the daemon. The helper continues independently. To watch progress, tail the install log from another process: `Get-Content -Wait "$env:LocalAppData\codeterm\self-install.log"`.

## Files

- `scripts/self-install-local.ps1` — standalone PowerShell version. Same flow as the CLI subcommand but invokable without a working daemon.

## Owner

`blackstar` — Windows-specific NSIS flow. Mac side has no equivalent; it would use Tauri's bundle-updater or `tauri-plugin-updater::install_update` for in-process installs.
