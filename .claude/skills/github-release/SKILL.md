---
title: End-to-end GitHub release for CodeTerm
area: deploy
tags: [release, github, macos, windows, notarize, brew, mesh]
---

# End-to-end CodeTerm release to GitHub

Full pipeline: bump version → build signed+notarized macOS (arm64+x86_64) → build Windows via mesh peer → publish to GitHub release → update Homebrew cask.

## When to use

User says: «сделай релиз», «выпусти vX.Y.Z», «ship it», «bump version + release», «assemble the binaries and put on github».

For just **building locally** (no release), use `prod-build`. For **just** the macOS x86_64 cross-compile gotchas, see `macos-x86_64-release`. This skill ties all three targets + upload + brew in one flow.

## Prereqs (verify once per machine)

- Apple signing identity `Developer ID Application: ANDREY KOVALEV (JMMWQ9LWF7)` in login keychain. Check: `security find-identity -v -p codesigning | grep ANDREY`.
- Notary keychain profile `codeterm`. If stale (`xcrun notarytool history --keychain-profile codeterm` → HTTP 401), re-store from the env file:
  ```bash
  source ~/.claude/.secrets/apple-notary.env
  xcrun notarytool store-credentials codeterm \
    --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
    --password "$APPLE_APP_SPECIFIC_PASSWORD"
  ```
  Note the env var is `APPLE_APP_SPECIFIC_PASSWORD`, NOT `APPLE_PASSWORD`.
- `gh` logged in as `rollacode`: `gh auth status`.
- BLACKSTAR mesh peer paired and online: `codeterm mesh peers | grep BLACKSTAR`.

## Steps

### 0. Kill any running dev

Rust builds share the target dir — concurrent `tauri build` + `tauri dev` fight and one silently fails:

```bash
./build.sh kill
```

### 1. Bump version in all four files (one commit)

```bash
VER=1.0.1  # new
OLD=1.0.0  # previous
sd "version = \"$OLD\"" "version = \"$VER\"" src-tauri/Cargo.toml
sd "\"version\": \"$OLD\"" "\"version\": \"$VER\"" src-tauri/tauri.conf.json package.json
./build.sh check                      # refreshes Cargo.lock too
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json package.json
git commit -m "release: v$VER"
```

If `sd` isn't installed, use `sed -i '' 's|…|…|g' <file>` (BSD sed on mac needs the empty `-i ''`).

### 2. Merge to master + tag

```bash
git push origin <feature-branch>
git checkout develop && git merge --no-ff <feature-branch> -m "merge: … into develop"
git push origin develop
git checkout master && git merge --no-ff develop -m "merge: v$VER from develop"
git tag -a v$VER -m "v$VER — <one-liner summary>"
git push origin master
git push origin v$VER
```

### 3. Build arm64 + notarize + staple

`./build.sh build` signs the inner `.app` and notarizes it, but **does NOT notarize the DMG wrapper itself**. Staple the DMG manually:

```bash
./build.sh build
ARM=src-tauri/target/release/bundle/dmg/CodeTerm_${VER}_aarch64.dmg
xcrun notarytool submit "$ARM" --keychain-profile codeterm --wait
xcrun stapler staple "$ARM"
spctl -a -t open --context context:primary-signature -vv "$ARM"  # must print "accepted, Notarized Developer ID"
```

### 4. Build x86_64 (full script handles notarize+staple)

```bash
bash scripts/release-macos-x86_64.sh
X64=src-tauri/target/x86_64-apple-darwin/release/bundle/dmg/CodeTerm_${VER}_x64.dmg
spctl -a -t open --context context:primary-signature -vv "$X64"   # must print accepted
```

Cross-compile detail (why this script exists): `macos-x86_64-release` skill.

### 5. Kick Windows build on BLACKSTAR via mesh

Spawn a claude agent in BLACKSTAR's `codeterm` workspace and let it pull the tag, build, and upload:

```bash
WORKSPACE_ID=$(codeterm --mesh BLACKSTAR --json workspace list \
  | python3 -c "import json,sys; print(next(w['id'] for w in json.load(sys.stdin) if w['name']=='codeterm'))")

codeterm --mesh BLACKSTAR agent spawn claude \
  --workspace "$WORKSPACE_ID" \
  --title "v$VER windows release" \
  --role Orchestrator --can-spawn \
  --task "You are on BLACKSTAR (Windows) via mesh. Build CodeTerm v$VER Windows release.

Steps:
1. cd C:\\Developer\\codeterm && git fetch origin && git checkout v$VER
2. pnpm install (if deps changed)
3. .\\build.sh build   (or ./build.sh build under bash)
4. NSIS exe lands at src-tauri\\target\\release\\bundle\\nsis\\CodeTerm_${VER}_x64-setup.exe
5. gh release upload v$VER <exe> --clobber
6. codeterm agent report STATUS/ASSETS/ISSUES

If pnpm install fails with 'UNKNOWN package.json' on Windows, rm -rf node_modules && pnpm install."
```

Alternative (what actually happened in v1.0.1): devops agent on dev machine SSH'd into BLACKSTAR and did the build directly via ssh + scp. Either works — mesh-spawn is more hands-off, SSH is more debuggable.

### 6. Upload macOS DMGs + publish release

```bash
gh release upload v$VER "$ARM" "$X64" --clobber
gh release edit v$VER --draft=false --latest
gh release view v$VER --json assets --jq '.assets[] | "\(.name)  \(.size)B"'
```

Expect 3 assets: `CodeTerm_${VER}_aarch64.dmg`, `CodeTerm_${VER}_x64.dmg`, `CodeTerm_${VER}_x64-setup.exe`.

### 7. Bump Homebrew cask

```bash
gh api repos/rollacode/codeterm/releases/tags/v$VER --jq '.assets[] | select(.name|endswith(".dmg")) | "\(.name) \(.digest)"'
# → captures two sha256 values; drop them into the cask

gh repo clone rollacode/homebrew-codeterm /tmp/homebrew-codeterm
cd /tmp/homebrew-codeterm && git pull --ff-only
# Edit Casks/codeterm.rb:
#   - bump version string
#   - replace on_arm sha256 with aarch64 digest
#   - replace on_intel sha256 with x64 digest
git add Casks/codeterm.rb
git commit -m "codeterm: bump to $VER"
git push origin main
```

Both repos are currently PRIVATE — `brew install codeterm` still won't work for the public (GitHub returns 404 for unauthenticated download URLs on private repos). Bumping the cask keeps the tap self-consistent for when repos go public.

### 8. Verify download URLs (private caveat)

```bash
gh release download v$VER --pattern "*" -D /tmp/v$VER_verify
ls -la /tmp/v$VER_verify/
```

`gh` uses the auth token so this works. Plain `curl` on the public URL returns 404 while the repo is private — that's expected, not a release problem.

## Failure modes seen in the wild

| Symptom | Cause | Fix |
|---------|-------|-----|
| `tauri build` ends with no DMG, no error | `build.sh dev` still running, Cargo target dir contention | `./build.sh kill` first |
| DMG `spctl` → "rejected, Unnotarized Developer ID" | tauri signed the `.app` but left the DMG wrapper unsigned notarization | Run `xcrun notarytool submit … --wait && xcrun stapler staple …` manually |
| `notarytool store-credentials` → HTTP 401 | keychain creds expired OR used `APPLE_PASSWORD` instead of `APPLE_APP_SPECIFIC_PASSWORD` | source the env file, use correct var name |
| BLACKSTAR build: `UNKNOWN error opening node_modules/sigma/package.json` | Windows filesystem race on freshly added dep | `rm -rf node_modules && pnpm install` on BLACKSTAR |
| SSH to BLACKSTAR: `cd /c/…` fails | `bash -lc` runs a login shell that re-homes | use `bash -c` (non-login) |
| `scp User@blackstar:/c/…` fails | remote shell is cmd.exe, doesn't grok the path | stage the file to `C:\Users\User\` on BLACKSTAR first, then `scp User@blackstar:filename` |
| Public `curl` of release asset → 404 | repo private, CDN rejects unauthenticated | use `gh release download`, or make repo public |

## Known gaps (fix opportunistically)

- `./build.sh build` should staple the DMG wrapper automatically — currently manual step in §3. Worth patching `build.sh` to mirror what `scripts/release-macos-x86_64.sh` already does.
- No CI release workflow — everything's local. A GitHub Action on tag push would replace §§3-6.
- Brew tap bump is manual — a `scripts/release-bump-brew.sh` could read the release JSON and rewrite the cask in one shot.
