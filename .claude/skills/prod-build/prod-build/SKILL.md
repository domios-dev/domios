---
title: Prod Build — CodeTerm production binary
area: build
tags: [build, prod, deploy]
owner: mac
scope: shared
source: git
---

# Prod Build — CodeTerm production binary

Build and deploy a prod CodeTerm binary on the developer's machine. Different host OS = different gotchas; record what you learn.

**Scope:** this is the "prod" counterpart to `dev-run` — for producing installable binaries / testing a real prod install, not the hot-reload dev loop.

## When to use

- User asks to "собери prod", "build prod", "deploy to prod", "replace the installed app", "положи в `AppData\Local\CodeTerm`".
- Fixing a bug that only reproduces in prod mode (resume flow, installer flow, bundled resources).
- Preparing a GitHub release asset.

## The one command

```bash
cd C:/Developer/codeterm
./build.sh build
```

This runs the full Tauri pipeline:
1. `pnpm build` (frontend → `dist/`)
2. `cargo build --release --manifest-path cli/Cargo.toml` (CLI)
3. `node scripts/bundle-cli.js` (CLI → `bundled-cli/`)
4. `cargo build --release --bin codeterm-embed` + `node scripts/bundle-embed.js` (sidecar → `bundled-embed/`)
5. `cargo tauri build` — compiles `codeterm.exe` with frontend EMBEDDED, then bundles NSIS/MSI/DMG.

**NEVER** run `cargo build --release` directly on `src-tauri/` to produce a prod binary. That skips steps 1-4 and the embedded-frontend wiring, yielding a binary that tries to load `http://localhost:1420` (devUrl) and shows `ERR_CONNECTION_REFUSED` in the window.

## Windows

### WIX MSI bundler often fails on this machine

Tauri 2 + `WixTools314/light.exe` produces:
```
failed to bundle project `failed to run C:\Users\User\AppData\Local\tauri\WixTools314\light.exe`
```
This is a known tauri/wix tooling issue — **not your code**.

**Workaround:** even when WIX fails, `codeterm.exe` (and `codeterm-embed.exe`) are already built in `src-tauri/target/release/` by the time WIX is invoked. The Rust binary is correct (frontend embedded, prod URL, sidecars packed as resources).

### Deploy path for iterating on prod

```bash
# 1. Kill running prod (port 7685)
powershell.exe -NoProfile -Command "Get-Process codeterm -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Seconds 2"

# 2. Copy fresh binaries into the installed location
cp C:/Developer/codeterm/src-tauri/target/release/codeterm.exe "$USERPROFILE/AppData/Local/CodeTerm/codeterm.exe"
cp C:/Developer/codeterm/src-tauri/target/release/codeterm-embed.exe "$USERPROFILE/AppData/Local/CodeTerm/codeterm-embed.exe"   # if you touched the sidecar

# 3. Restart
powershell.exe -NoProfile -Command "Start-Process -FilePath 'C:\Users\User\AppData\Local\CodeTerm\codeterm.exe'"
```

After launch, confirm the build number you expect:
```bash
curl -s http://127.0.0.1:7685/api/health
# {"build_number":"156", ...}
```

If the window shows `ERR_CONNECTION_REFUSED` for localhost → your build was compiled in dev mode. Go back to `./build.sh build`.

### Ship to the user's desktop

```bash
cp C:/Developer/codeterm/src-tauri/target/release/bundle/nsis/CodeTerm_0.10.0_x64-setup.exe "$USERPROFILE/Desktop/"
```

(Only exists if the NSIS leg of bundling succeeded — if WIX failed first, NSIS may not have run.)

### Upload to a GitHub release

```bash
cd C:/Developer/codeterm
gh release view v0.10.0 --json assets -q '.assets[].name'          # check current assets
gh release delete-asset v0.10.0 <name> --yes                       # remove stale Windows assets
gh release upload v0.10.0 \
  src-tauri/target/release/bundle/nsis/CodeTerm_0.10.0_x64-setup.exe \
  src-tauri/target/release/bundle/msi/CodeTerm_0.10.0_x64_en-US.msi
```

Don't upload macOS assets from a Windows build — those must come from the macOS build machine.

## macOS

_No entries yet. Add as they're discovered._

If you work on mac, confirm:
- Whether `./build.sh build` works end-to-end or if something needs `brew install create-dmg` / rosetta / codesign identity setup.
- Whether the sidecar codesigning hook in `scripts/bundle-embed.js` runs successfully (it checks `APPLE_SIGNING_IDENTITY`).
- Where the installed binary lives (`/Applications/CodeTerm.app/Contents/MacOS/codeterm`).
- The kill command (`pkill -x codeterm` should be enough).

## Linux

_No entries yet. Add as they're discovered._

## Rule: grow this skill

Whenever you hit something platform-specific or a tooling quirk that bit you during a prod build:
1. Add a dated entry under the relevant OS section.
2. Note symptom → root cause → fix.
3. If the fix is a one-liner command, include it verbatim so future-you just copy-pastes.

Don't record info already in the main build.sh script or in `CLAUDE.md` — link to them instead. Record the *friction points* that those docs don't cover.
