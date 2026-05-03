---
title: End-to-end GitHub release for CodeTerm
area: deploy
tags: [release, github, macos, windows, notarize, brew, mesh, mirror]
---

# End-to-end CodeTerm release to GitHub

Full pipeline: bump version on develop → merge to master → tag → checkout tag → build signed+notarized macOS (arm64+x86_64) → build Windows via mesh → upload to BOTH the private source repo AND the public release mirror → sign + publish updater manifest → update Homebrew cask.

**Golden rule: all builds happen from the tagged commit on master. Never build release artifacts from develop.**

**Two-repo invariant: every artifact and the signed `latest.json` MUST land in `rollacode/codeterm-releases` (PUBLIC mirror) or anonymous downloads will 404 and the auto-updater will silently fail. The private `rollacode/codeterm` source repo gets the same payload for tooling that pulls from there, but the mirror is the one users actually consume.**

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

### 1. Bump version on develop (one commit)

```bash
git checkout develop
VER=1.2.0  # new
OLD=1.1.2  # previous
sed -i '' "s/^version = \"$OLD\"/version = \"$VER\"/" src-tauri/Cargo.toml
sed -i '' "s/\"version\": \"$OLD\"/\"version\": \"$VER\"/g" src-tauri/tauri.conf.json package.json
./build.sh check                      # refreshes Cargo.lock
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json package.json
git commit -m "release: v$VER"
git push origin develop
```

### 2. Merge to master + tag + checkout tag

```bash
git checkout master
git pull origin master --no-rebase
git merge --no-ff develop -m "merge: v$VER from develop"
git tag -a v$VER -m "v$VER — <one-liner summary>"
git push origin master
git push origin v$VER
```

If push to master is rejected (non-fast-forward), always `git pull origin master --no-rebase --no-edit` first, then push again.

### 3. Create GitHub release (draft, on BOTH repos)

```bash
NOTES_BODY="$(cat <<'NOTES'
## Changes
- ...
NOTES
)"
gh release create v$VER --repo rollacode/codeterm           --title "v$VER" --draft --notes "$NOTES_BODY"
gh release create v$VER --repo rollacode/codeterm-releases  --title "v$VER" --draft --notes "$NOTES_BODY"
```

Create as draft on both. Publish after all assets are uploaded and the updater manifest is signed.

### 4. Checkout tag for building

**Critical: build from the tag, not from a branch.**

```bash
git checkout v$VER
```

You're now in detached HEAD on the exact tagged commit. All builds below use this tree.

### 5. Build arm64 + notarize + staple

`./build.sh build` signs the inner `.app` and notarizes it, but **does NOT notarize the DMG wrapper itself**. Staple the DMG manually:

```bash
./build.sh build
ARM=src-tauri/target/release/bundle/dmg/CodeTerm_${VER}_aarch64.dmg
xcrun notarytool submit "$ARM" --keychain-profile codeterm --wait
xcrun stapler staple "$ARM"
spctl -a -t open --context context:primary-signature -vv "$ARM"  # must print "accepted, Notarized Developer ID"
```

### 6. Build x86_64 (full script handles notarize+staple)

```bash
bash scripts/release-macos-x86_64.sh
X64=src-tauri/target/x86_64-apple-darwin/release/bundle/dmg/CodeTerm_${VER}_x64.dmg
spctl -a -t open --context context:primary-signature -vv "$X64"   # must print accepted
```

Cross-compile detail (why this script exists): `macos-x86_64-release` skill.

### 7. Upload macOS DMGs (BOTH repos)

```bash
for f in "$ARM" "$X64"; do
  gh release upload v$VER "$f" --clobber --repo rollacode/codeterm
  gh release upload v$VER "$f" --clobber --repo rollacode/codeterm-releases
done
```

### 8. Kick Windows build on BLACKSTAR via mesh

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
5. Upload to BOTH repos:
   gh release upload v$VER <exe> --clobber --repo rollacode/codeterm
   gh release upload v$VER <exe> --clobber --repo rollacode/codeterm-releases
6. codeterm agent report STATUS/ASSETS/ISSUES

If pnpm install fails with 'UNKNOWN package.json' on Windows, rm -rf node_modules && pnpm install."
```

Alternative: devops agent on dev machine SSH'd into BLACKSTAR and did the build directly via ssh + scp. Either works — mesh-spawn is more hands-off, SSH is more debuggable.

### 8b. Sign each asset with minisign + publish updater manifest (mirror only)

The auto-updater fetches `latest.json` from the public mirror and verifies it against the ed25519/minisign pubkey baked into `tauri.conf.json::plugins.updater.pubkey`. Sign every platform asset and upload the `.sig` files + a per-platform `latest.json` to the mirror:

```bash
# Sign each artifact
for f in "$ARM" "$X64" "CodeTerm_${VER}_x64-setup.exe"; do
  minisign -Sm "$f" -s ~/.tauri/codeterm.key
  gh release upload v$VER "$f.sig" --clobber --repo rollacode/codeterm-releases
done

# Build latest.json with windows-x86_64, darwin-aarch64, darwin-x86_64 entries pointing
# at the mirror URLs and the inline minisign signatures, then upload.
# scripts/build-latest-json.sh is the canonical generator.
bash scripts/build-latest-json.sh v$VER > latest.json
gh release upload v$VER latest.json --clobber --repo rollacode/codeterm-releases
```

`latest.json` lives ONLY in the mirror. The private repo doesn't need it because the updater endpoint resolves to the mirror.

### 9. Publish release + verify (BOTH repos)

```bash
gh release edit v$VER --draft=false --latest --repo rollacode/codeterm
gh release edit v$VER --draft=false --latest --repo rollacode/codeterm-releases

# Verify mirror — anonymous curl must succeed without auth
curl -sS -o /dev/null -w "%{http_code}\n" \
  https://github.com/rollacode/codeterm-releases/releases/latest/download/latest.json   # 200
curl -sS -o /dev/null -w "%{http_code}\n" \
  https://github.com/rollacode/codeterm-releases/releases/download/v$VER/CodeTerm_${VER}_x64-setup.exe   # 302 → 200

gh release view v$VER --repo rollacode/codeterm-releases --json assets --jq '.assets[] | "\(.name)  \(.size)B"'
```

Expect on the mirror: 3 binaries (`CodeTerm_${VER}_aarch64.dmg`, `CodeTerm_${VER}_x64.dmg`, `CodeTerm_${VER}_x64-setup.exe`), 3 `.sig` files, plus `latest.json`. The private source repo holds the same 3 binaries (no .sig, no latest.json — those live only on the mirror).

### 10. Return to develop

```bash
git checkout develop
```

### 11. Bump Homebrew cask (optional, repos still private)

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

Both repos are currently PRIVATE — `brew install codeterm` still won't work for the public. Bumping the cask keeps the tap self-consistent for when repos go public.

## Failure modes seen in the wild

| Symptom | Cause | Fix |
|---------|-------|-----|
| `tauri build` ends with no DMG, no error | `build.sh dev` still running, Cargo target dir contention | `./build.sh kill` first |
| DMG `spctl` → "rejected, Unnotarized Developer ID" | tauri signed the `.app` but left the DMG wrapper unsigned | Run `xcrun notarytool submit … --wait && xcrun stapler staple …` manually |
| `notarytool store-credentials` → HTTP 401 | keychain creds expired OR used `APPLE_PASSWORD` instead of `APPLE_APP_SPECIFIC_PASSWORD` | source the env file, use correct var name |
| `git push origin master` → non-fast-forward | master diverged (e.g. previous release merged differently) | `git pull origin master --no-rebase --no-edit` then push again |
| BLACKSTAR build: `UNKNOWN error opening node_modules/sigma/package.json` | Windows filesystem race on freshly added dep | `rm -rf node_modules && pnpm install` on BLACKSTAR |
| SSH to BLACKSTAR: `cd /c/…` fails | `bash -lc` runs a login shell that re-homes | use `bash -c` (non-login) |
| `scp User@blackstar:/c/…` fails | remote shell is cmd.exe, doesn't grok the path | stage the file to `C:\Users\dev\` on BLACKSTAR first, then `scp User@blackstar:filename` |
| Public `curl` of release asset → 404 | repo private, CDN rejects unauthenticated | use `gh release download`, or make repo public |

## Known gaps (fix opportunistically)

- `./build.sh build` should staple the DMG wrapper automatically — currently manual step in §5. Worth patching `build.sh` to mirror what `scripts/release-macos-x86_64.sh` already does.
- No CI release workflow — everything's local. A GitHub Action on tag push would replace §§5-8.
- Brew tap bump is manual — a `scripts/release-bump-brew.sh` could read the release JSON and rewrite the cask in one shot.
