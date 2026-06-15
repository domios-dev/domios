---
title: End-to-end GitHub release for CodeTerm
area: deploy
tags: [release, github, macos, windows, notarize, brew, mesh]
owner: mac
scope: shared
source: git
---


# End-to-end CodeTerm release to GitHub

Full pipeline: bump version on develop → merge to master → tag → checkout tag → build signed+notarized macOS (arm64+x86_64) → build Windows via mesh → upload to private repo (source) → mirror artifacts + signed manifest to public repo (so GUI updater works) → update Homebrew cask.

**Golden rule: all builds happen from the tagged commit on master. Never build release artifacts from develop.**

## Two-repo layout (since 2026-05-03)

- **`rollacode/codeterm`** — private source repo. Releases here for archive only; the GUI updater can't reach private-repo assets anonymously.
- **`rollacode/codeterm-releases`** — public mirror, artifacts + `latest.json` only. The Tauri-plugin-updater endpoint baked into `tauri.conf.json` points here. Without this mirror, `update install --auto` from the GUI hangs/404s.

## When to use

User says: «сделай релиз», «выпусти vX.Y.Z», «ship it», «bump version + release», «assemble the binaries and put on github».

For just **building locally** (no release), use `prod-build`. For **just** the macOS x86_64 cross-compile gotchas, see `macos-x86_64-release`. This skill ties all three targets + upload + brew in one flow.

## Agent report contract — machine-token first line

Every worker that finishes a step reports back via `codeterm agent report`. The orchestrator parses the FIRST LINE of the report as a structured event, never regex-greps the prose. Sourced from `scripts/lib/report.sh` which both mac and win helpers `source`:

| Outcome  | First line                                     |
| -------- | ---------------------------------------------- |
| Ready    | `WINDOWS_READY=<ver> sig=<base64>` (or similar key=val pairs) |
| Blocked  | `BLOCKED step=<name> err=<short>`              |

A blank line separates the token from free-form prose. Helpers:

- `report_ready KEY=VAL ...` — emits the kv line + `\n\n` + stdin prose
- `report_blocked STEP MSG` — emits `BLOCKED step=… err=…` + `\n\n` + stdin prose
- `validate_b64_sig RAW` — echoes the trimmed sig if it looks valid (≥200 chars, only b64 alphabet), exits non-zero otherwise. Wrap the output of any `*-bundle.sh` before reporting it.

Reason: v1.3.2 release wasted a round-trip because BLACKSTAR reported "sig: \<base64\>" inside prose and the mac orchestrator couldn't extract it without human relay. Machine token on line 1 fixes that class permanently.

## Release-pipeline secrets backup

Stored in Bitwarden vault (retrieve via `codeterm mem secret get --name <slug>` on any machine with BW unlocked):

| Bitwarden slug | Restore to | Notes |
|---|---|---|
| `codeterm-apple-notary-env` | `~/.claude/.secrets/apple-notary.env` (`chmod 600`) | Then `source <file>; xcrun notarytool store-credentials codeterm --apple-id $APPLE_ID --team-id $APPLE_TEAM_ID --password $APPLE_APP_SPECIFIC_PASSWORD` |
| `codeterm-tauri-minisign-private-key` | `~/.tauri/codeterm.key` (`chmod 600`) | Empty password. Used by `tauri signer sign` for updater artifacts |
| `codeterm-tauri-minisign-public-key` | `~/.tauri/codeterm.key.pub` (`chmod 644`) | Same value already baked into `src-tauri/tauri.conf.json::plugins.updater.pubkey` |

**NOT in the vault** (live only in macOS Login Keychain):
- `Developer ID Application: ANDREY KOVALEV (JMMWQ9LWF7)` certificate + private key. Back up via: Keychain Access → find by name → File → Export as `.p12` with passphrase → save `.p12` + passphrase to mem secret as `codeterm-apple-developer-id-p12`.

`codeterm mem secret status` must show `unlocked`; `codeterm mem secret list` shows everything.

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
- Windows builder mesh peer paired and online: `codeterm mesh peers`. Default = **BLACKSTAR** (Windows 11, has its own minisign key copy since 2026-05-18, workspace at `C:\Developer\codeterm`). Fallback = **ROG** (primary workstation, Windows 11, `100.64.10.9`, codeterm workspace at `C:\Users\dev\Developer\codeterm`, workspace id `28fa3d8d`).
- **Windows builder prereqs** (verify on the chosen peer before invoking `win-release.sh`):
  - `gh auth status` shows `rollacode (keyring)` as **valid**, not just `r0llac0de (work)` as the active account. The work account has happened to retain push access to `rollacode/codeterm` (so uploads can succeed even when rollacode keyring is stale), but this is fragile — refresh via `gh auth login` if rollacode is invalid. *(v1.2.1 caught: rollacode keyring expired silently; r0llac0de was the actual writer.)*
  - `node --version` ≥ 22.12 (Vite 8 hard requirement). If lower, `nvm use 22.22.0` (or whichever 22.12+ is installed). nvm-windows shim is usually not on PATH by default — prepend the version dir manually if `nvm use` doesn't work.
  - Windows SDK matches what `build.sh` expects (default `SDK_VER=10.0.22621.0`). Check `ls "/c/Program Files (x86)/Windows Kits/10/Include"`. On ROG only `10.0.26100.0` is installed → build.sh has a local stash with the override; restore via `git stash pop` after `git checkout v$VER`. A clean ROG without that stash dead-ends on `sqlite-sys: malloc.h not found`.

## Steps

### Agent model

When delegating this skill to an agent (release coordinator or ROG worker), **spawn with `--model haiku`**. The pipeline is mostly orchestration + bash + git/gh — Haiku is fast and cheap enough; Sonnet/Opus is overkill and doubles wall time on thinking phases.

Spawn with `codeterm agent spawn claude --task "..." --model haiku`. Parent is inferred from `$CODETERM_PANE_ID`; reports route back to you automatically (Orchestrator + sub-pane model, no `--role` flag needed).

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

# Verify all three files match $VER. The sed above sometimes misses (different
# quote styles, OLD value wrong, etc.) and the bug isn't caught until BLACKSTAR
# builds CodeTerm_1.1.8_x64-setup.exe with the stale version in the filename.
# v1.1.9 burned 15min of build time on this; never again.
grep "\"version\": \"$VER\"" src-tauri/tauri.conf.json package.json
grep "^version = \"$VER\"" src-tauri/Cargo.toml
# All three lines must print a match. If not, investigate before pushing the tag.

git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json package.json
git commit -m "release: v$VER"
git push origin develop
```

### 2. Merge to master + tag + checkout tag

**Always via PR on GitHub** — don't direct-push develop to master. This avoids merge-commit collisions when multiple machines operate on master.

```bash
# On GitHub: open PR develop → master, merge via "Create a merge commit"
# Then locally:
git checkout master
git pull origin master --no-rebase   # get the merge commit GitHub created
git tag -a v$VER -m "v$VER — <one-liner summary>"
git push origin v$VER
```

If you need to merge post-release develop commits to master (docs, skills, etc.) after the tag:
```bash
git checkout master && git pull origin master --no-rebase
git merge --no-ff origin/develop --no-edit
git push origin master
```

If push to master is rejected: always `git pull origin master --no-rebase --no-edit` first — origin/master may already have a merge commit from a GitHub PR that your local doesn't have yet.

### 3. Create GitHub release (draft)

```bash
gh release create v$VER --title "v$VER" --draft --notes "$(cat <<'NOTES'
## Changes
- ...
NOTES
)"
```

Create as draft first. Publish after all assets are uploaded.

### 4. Checkout tag for building

**Critical: build from the tag, not from a branch.**

```bash
git checkout v$VER
```

You're now in detached HEAD on the exact tagged commit. All builds below use this tree.

### 5+6. Build + notarize + pack updater bundles (both archs)

```bash
WORK=/tmp/codeterm-${VER}-mirror
mkdir -p "$WORK"
HELPERS=.codeterm/skills/github-release/scripts
ARM_SIG=$("$HELPERS/mac-bundle.sh" "$VER" arm64   "$WORK")
X64_SIG=$("$HELPERS/mac-bundle.sh" "$VER" x86_64 "$WORK")
```

`mac-bundle.sh` does the whole arch-pipeline per call:
1. `build-mac.sh` — sources `~/.claude/.secrets/apple-notary.env`, kills any running dev, runs the build, verifies the resulting `.app` was signed with `Developer ID Application` + `flags=0x10000(runtime)`. Failing this sanity check beats discovering an ad-hoc-signed binary half an hour into Apple's notarize queue. (Hard-learned 2026-05-17 v1.3.0.)
2. `notarize-dmg.sh` — submits, waits (quiet mode: dot-spam suppressed), staples, runs `spctl`. On rejection prints Apple's detailed issues to stderr.
3. `pack-mac-updater.sh` — `tar --no-mac-metadata` (required: without it Tauri's Rust tar lib materializes `._CodeTerm.app` AppleDouble shadows on extract and the updater rejects with "failed to unpack `._CodeTerm.app`"), Python-extract sanity check, minisign-sign. Echoes the base64 signature to stdout.

The captured `ARM_SIG` / `X64_SIG` go straight into latest.json below.

### Status dashboard (any time during the flow)

```bash
.codeterm/skills/github-release/scripts/release-status.sh "$VER"
```

Compact one-shot view of: private/public assets, latest.json manifest plat coverage, and per-platform CI run state. Replaces 3-4 separate `gh` queries.

Cross-compile detail (why this script exists): `macos-x86_64-release` skill.

### 7. Upload macOS DMGs to the private (source) repo

```bash
gh release upload v$VER "$ARM" "$X64" --clobber
```

### 7b. Sign DMGs + build `latest.json` + mirror to public repo

The GUI updater fetches `latest.json` anonymously, so the manifest plus the asset copies live in `rollacode/codeterm-releases`. Same files, separate repo.

**⚠️ Format gotcha (learned the hard way on v1.1.17): the `tauri-plugin-updater` does NOT consume raw `.dmg` / `.exe` files.** It expects:

- macOS: `<name>_<ver>_<arch>.app.tar.gz` (the `.app` bundle wrapped in gzipped tar)
- Windows: `<name>_<ver>_x64-setup.nsis.zip` (the NSIS installer wrapped in a zip)

Pointing `latest.json` at `.dmg` / `.exe` URLs makes `codeterm update install --auto` fail with `invalid gzip header` on macOS and an analogous unpack error on Windows. The user-facing direct downloads (`.dmg` / `.exe`) still belong on the mirror — they are useful for first-install / out-of-band install — but the updater URLs in `latest.json` MUST point at the `.app.tar.gz` / `.nsis.zip` artifacts. Keep both formats side-by-side on the release.

```bash
WORK=/tmp/codeterm-${VER}-mirror
mkdir -p "$WORK" && cp "$ARM" "$X64" "$WORK/"

# 1. Mac updater bundles + signatures came out of `mac-bundle.sh` above as
#    $ARM_SIG / $X64_SIG. The .tar.gz files are in $WORK; .sig files are
#    persisted alongside for archive but NOT uploaded to the release.

# 2. Sign each updater artifact with the minisign key (key file at ~/.tauri/codeterm.key,
# base64-encoded minisign secret, password empty for the local-dev key). The .sig is
# READ once below to extract the base64 signature into latest.json, then thrown away —
# the tauri updater reads `signature` from the manifest, never downloads .sig files.
# The .dmg / .exe files do NOT need signing — they are user-facing only.
KEY_B64=$(cat ~/.tauri/codeterm.key | tr -d '\n')
for art in \
  "$WORK"/CodeTerm_${VER}_aarch64.app.tar.gz \
  "$WORK"/CodeTerm_${VER}_x64.app.tar.gz; do
  TAURI_SIGNING_PRIVATE_KEY="$KEY_B64" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
    node_modules/.bin/tauri signer sign "$art"
done
# NOTE: use `node_modules/.bin/tauri`, NOT `npx --yes @tauri-apps/cli signer sign`
# — npx fails with "Unknown command: @tauri-apps/cli" on Windows and is brittle
# on macOS too. Direct binary always works.

# 3. The Windows mesh worker (win-release.sh on BLACKSTAR / ROG) signs its own
# .nsis.zip with the same minisign key (~/.tauri/codeterm.key, restored from
# Bitwarden slug codeterm-tauri-minisign-private-key). Since v1.3.3 the Tauri
# bundler emits CodeTerm_${VER}_x64-setup.nsis.zip alongside the .exe automatically
# (bundle.createUpdaterArtifacts = true in tauri.conf.json) — the worker's
# Compress-Archive fallback only fires if that flag is ever turned off.

# 4. Optional verify (requires `brew install minisign`):
# base64 -D < "$WORK"/CodeTerm_${VER}_aarch64.app.tar.gz.sig > /tmp/check.minisig
# echo "<pubkey base64 from tauri.conf.json>" | base64 -D > /tmp/check.pub
# minisign -V -p /tmp/check.pub -m "$WORK"/CodeTerm_${VER}_aarch64.app.tar.gz

# 5. Build the manifest. Schema = standard tauri-plugin-updater shape.
# URLs MUST point at .app.tar.gz / .nsis.zip — not raw .dmg / .exe.
ARM_SIG=$(cat "$WORK/CodeTerm_${VER}_aarch64.app.tar.gz.sig" | tr -d '\n')
X64_SIG=$(cat "$WORK/CodeTerm_${VER}_x64.app.tar.gz.sig"     | tr -d '\n')
WIN_SIG=$(cat "$WORK/CodeTerm_${VER}_x64-setup.nsis.zip.sig" | tr -d '\n' 2>/dev/null || echo "")
PUB_DATE=$(gh release view v$VER --json publishedAt --jq '.publishedAt')
NOTES=$(gh release view v$VER --json body --jq '.body')
cat > "$WORK/latest.json" <<JSON
{
  "version": "$VER",
  "notes": $(printf '%s' "$NOTES" | python3 -c 'import sys,json; print(json.dumps(sys.stdin.read()))'),
  "pub_date": "$PUB_DATE",
  "platforms": {
    "darwin-aarch64": {
      "url": "https://github.com/rollacode/codeterm-releases/releases/latest/download/CodeTerm_${VER}_aarch64.app.tar.gz",
      "signature": "$ARM_SIG"
    },
    "darwin-x86_64": {
      "url": "https://github.com/rollacode/codeterm-releases/releases/latest/download/CodeTerm_${VER}_x64.app.tar.gz",
      "signature": "$X64_SIG"
    },
    "windows-x86_64": {
      "url": "https://github.com/rollacode/codeterm-releases/releases/latest/download/CodeTerm_${VER}_x64-setup.nsis.zip",
      "signature": "$WIN_SIG"
    }
  }
}
JSON
python3 -m json.tool "$WORK/latest.json" >/dev/null  # validate

# 6. Publish on the public mirror — upload the user-facing direct downloads
# (.dmg / .exe) AND the updater artifacts (.app.tar.gz / .nsis.zip). Do NOT upload
# the .sig files — their content already lives inline in latest.json's `signature`
# field, and the tauri updater never fetches separate .sig assets. Same rule for
# any standalone CLI binary: it's already bundled inside the .app / .exe as a
# sidecar, so no separate cli archive on the release.
gh release create v$VER \
  --repo rollacode/codeterm-releases \
  --title "v$VER" --target main \
  --notes "Release artifacts for CodeTerm v$VER. Source: https://github.com/rollacode/codeterm/releases/tag/v$VER (private)." \
  --latest \
  "$WORK/CodeTerm_${VER}_aarch64.dmg" \
  "$WORK/CodeTerm_${VER}_x64.dmg" \
  "$WORK/CodeTerm_${VER}_x64-setup.exe" \
  "$WORK/CodeTerm_${VER}_aarch64.app.tar.gz" \
  "$WORK/CodeTerm_${VER}_x64.app.tar.gz" \
  "$WORK/CodeTerm_${VER}_x64-setup.nsis.zip" \
  "$WORK/latest.json"

# 7. Verify anon access (smoke):
curl -sIL https://github.com/rollacode/codeterm-releases/releases/latest/download/latest.json \
  | grep -E '^HTTP' | tail -1     # → HTTP/2 200

# 8. End-to-end smoke: a freshly-installed v$VER- daemon should pick this up.
# Run on the build host (after self-installing v$VER- via codeterm self-update or DMG):
codeterm update check       # → 'Update available!' OR 'Up to date' (latest manifest is current)
# On the previous-version host (still on v<previous>):
codeterm update install --auto     # → must NOT fail with 'invalid gzip header'.
                            # If it does, the manifest is still wrong — fix and re-publish before continuing.
```

If the Windows build is still pending when you mirror, leave the `windows-x86_64.signature` placeholder ("") and update the manifest after BLACKSTAR uploads its signed `.nsis.zip.sig` (re-upload `latest.json` with `--clobber`). Same flow applies if BLACKSTAR delivers `.nsis.zip` AFTER you have already published the public mirror — just re-upload `latest.json` with the corrected windows signature.

### 8. Windows build

**Default path: dispatch a worker on a paired Windows mesh peer** running `win-release.sh`. The script is the single end-to-end Windows pipeline (checkout → install → bundle → sign → upload → machine-parseable agent report). BLACKSTAR is the primary builder; ROG is a fallback with the same flow.

```bash
WORKSPACE_ID=$(codeterm --mesh BLACKSTAR --json workspace list \
  | python3 -c "import json,sys; print(next(w['id'] for w in json.load(sys.stdin) if w['name']=='codeterm'))")

codeterm --mesh BLACKSTAR agent spawn claude \
  --workspace "$WORKSPACE_ID" \
  --title "v$VER windows release" \
  --role Orchestrator \
  --model haiku \
  --task "bash .codeterm/skills/github-release/scripts/win-release.sh $VER"
```

The agent's final `codeterm agent report` line is parseable:

```
WINDOWS_READY=1.3.3 sig=dW50cnVzdGVkIGNvbW1lbnQ6...

Windows release v1.3.3 built, signed, and uploaded to both repos.
```

Grep the first line for `WINDOWS_READY=` (success) or `BLOCKED step=` (failure) — the orchestrator does NOT need to read the prose.

What the script does internally (`scripts/win-bundle.sh` + `win-upload.sh`):

1. `git -c core.autocrlf=false checkout v$VER` (CRLF phantom-dirt workaround on Windows is built in).
2. `pnpm install --frozen-lockfile`.
3. **Export `TAURI_SIGNING_PRIVATE_KEY` BEFORE `./build.sh build`.** Without this env var, the Tauri bundler silently SKIPS the `.nsis.zip` and we fall back to repacking — historically this trap shipped DEFLATE-compressed zips that tauri-updater rejects (`Compression method not supported`). Caught the project on v1.1.22 AND v1.3.2 because the env-export was only done at the sign step. As of 2026-05-20 `win-bundle.sh` exports both `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` at script entry, so the bundler emits a proper STORED-compressed updater bundle the first time.
4. `./build.sh build` (NSIS bundler emits `.exe` + `.nsis.zip` natively when the key env is exported).
5. If the bundler still skips the `.nsis.zip` (very old tauri-cli), fallback uses `python zipfile.ZIP_STORED` — NEVER `powershell Compress-Archive`, whose `-CompressionLevel Optimal` always writes DEFLATE.
6. **Verify compression method bytes 8-9 of the local file header are `00 00` (STORED) before signing.** `win-bundle.sh` does this and exits with `BLOCKED:` on any other value. Manual check: `od -A x -t x1 -N 10 CodeTerm_*_x64-setup.nsis.zip` — bytes at offset 8-9 must be `00 00`. `08 00` = DEFLATE (broken), `09 00` = Deflate64 (broken).
7. Signs `.nsis.zip` via `node_modules/.bin/tauri signer sign` (NOT `npx --yes @tauri-apps/cli signer sign` — npx fails with "Unknown command" on Windows).
8. Idempotent `gh release view || gh release create --draft` for both repos, then `gh release upload --clobber` for `.exe` + `.nsis.zip`.

Wall times (BLACKSTAR, cache-warm): ~3min build, +~30s install if `node_modules` cold, +~10s upload. Cold from a fresh `pnpm install`: ~5-7min total.

**CI path (`release-windows.yml`)**: still triggers automatically on tag push and uploads `.exe` + `.nsis.zip` to the private repo. The mesh-worker flow is preferred because BLACKSTAR has the minisign key locally and can sign in one shot — CI does not have access to `~/.tauri/codeterm.key` so its outputs need to be re-signed manually anyway. Treat CI artifacts as a back-up source for `gh release download` when the worker is offline.

Direct local invocation (on a Windows machine with a checkout, e.g. for testing the pipeline without dispatching an agent):

```bash
# In Git Bash on BLACKSTAR / ROG:
bash .codeterm/skills/github-release/scripts/win-release.sh $VER
# stdout is the machine-parseable agent report
```

ROG-specific gotchas (still relevant if you fall back to ROG):
- Local SDK pinning (`build.sh` SDK_VER + `@rolldown/binding-win32-x64-msvc` 1.0.0-rc.9). Stored in a local `git stash` on the machine; `win-release.sh` does not auto-pop it — manual `git stash pop` if you see "SDK not found".
- `nvm use 22.22.0` (or any 22.12+) BEFORE invocation — Vite 8 fails on lower Node.
- gh authed as `rollacode (keyring)` should be valid; `r0llac0de (work)` is the work fallback and has push access too.

Alternative paths (debugging tactics, not normal flow):
- Send to an existing General Agent pane via `codeterm --mesh BLACKSTAR send-text --pane <general> --from-file <msg.txt>` to bypass `agent spawn` argv-split bugs.
- Open a parallel shell pane on the builder with `pane new --workspace` for tactical interventions while the agent runs the long compile.

### 9. Publish release + verify

```bash
# Private source repo:
gh release edit v$VER --draft=false --latest
gh release view v$VER --json assets --jq '.assets[] | "\(.name)  \(.size)B"'
# Expect 4-5 assets on the source repo: aarch64.dmg, x64.dmg, x64-setup.exe + the
# Linux artifacts (`amd64.deb`, `amd64.AppImage`) when CI shipped them via release-linux.yml.
# The mirror (codeterm-releases) holds the same set plus `.app.tar.gz`, `.nsis.zip`, and `latest.json`.

# Public mirror — once Windows .exe arrives, also update its sig in latest.json
# and re-upload --clobber. Verify GUI updater can resolve anonymously:
curl -sIL https://github.com/rollacode/codeterm-releases/releases/latest/download/latest.json | grep '^HTTP' | tail -1
```

### 10. Sync develop with master + return to develop

After the release commit + tag land on master, `develop` is behind by the release/merge commits (and any docs/skill tweaks the release agent committed directly on master during the run). Fast-forward develop and push so the next release doesn't start from a stale base.

```bash
git checkout develop
git pull --ff-only origin master   # fast-forward; master == develop + release commits
git push origin develop
```

If `--ff-only` refuses (someone committed to develop during the release window), `git merge --no-ff origin/master --no-edit && git push origin develop`. **Don't skip this** — v1.2.1 forgot it and the next release had to re-discover the missing commits manually.

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

### 12. Regenerate translated READMEs from README.md (agent pass) translations must not ship stale

## Retag in-place (post-publish hotfix)

When a critical bug surfaces shortly after publish — **before users have widely auto-updated** — it's safe to force-move the tag and re-upload artifacts under the same version rather than bumping. Used for v1.1.15 (2026-05-05).

```bash
# 1. Commit fix on develop, merge --no-ff to master:
git checkout develop && git push origin develop
git checkout master && git merge --no-ff develop -m "merge: v$VER hotfix"
git push origin master

# 2. Move the tag:
git tag -f -a v$VER -m "v$VER — retag: <fix description>"
git push origin v$VER --force

# 3. Checkout the new tag commit (now points at the merge):
git checkout v$VER

# 4. Rebuild + notarize + staple all 3 platforms (steps 5–8 above).

# 5. Re-upload to private repo with --clobber:
gh release upload v$VER "$ARM" "$X64" "$WIN_EXE" --clobber

# 6. Fetch existing latest.json BEFORE patching, to preserve any platform's signature
#    you are NOT rebuilding (e.g. if you only re-ran macOS, keep the Windows sig):
curl -sLO https://github.com/rollacode/codeterm-releases/releases/download/v$VER/latest.json
#    ... patch only the rebuilt platforms ...
gh release upload v$VER --repo rollacode/codeterm-releases <new files + latest.json> --clobber

# 7. Bump brew cask sha256 (DMG bytes change with every rebuild even if content is equivalent).
```

**Constraint:** only safe while the audience is small/internal. Anyone who already auto-updated to the broken artifact keeps the broken binary. For external user-facing releases, always bump version instead.

Force-retag pre-publish is unilaterally allowed per `rule_blackstar_mac_release_sync_protocol`.

## Failure modes seen in the wild

| Symptom | Cause | Fix |
|---------|-------|-----|
| `tauri build` ends with no DMG, no error | `build.sh dev` still running, Cargo target dir contention | `./build.sh kill` first |
| DMG `spctl` → "rejected, Unnotarized Developer ID" | tauri signed the `.app` but left the DMG wrapper unsigned | Run `xcrun notarytool submit … --wait && xcrun stapler staple …` manually |
| `notarytool store-credentials` → HTTP 401 | keychain creds expired OR used `APPLE_PASSWORD` instead of `APPLE_APP_SPECIFIC_PASSWORD` | source the env file, use correct var name |
| `git push origin master` → non-fast-forward | master diverged (e.g. previous release merged differently) | `git pull origin master --no-rebase --no-edit` then push again |
| `codeterm update install --auto` → "invalid gzip header" | `latest.json` URLs point at raw `.dmg`/`.exe` instead of `.app.tar.gz`/`.nsis.zip` | rebuild manifest with wrapper URLs — see Step 7b. Wrappers are the ONLY format tauri-plugin-updater accepts |
| tauri-plugin-updater unpack on Windows → "Compression method not supported" | `.nsis.zip` built with 7z default (Deflate64); zip-rs only supports Deflate/Store | regenerate with `7z a -tzip -mm=Copy` (Store compression) |
| tauri-plugin-updater unpack on macOS → "failed to unpack ._CodeTerm.app" | macOS `tar` includes AppleDouble metadata files (`._*`) by default | `COPYFILE_DISABLE=1 tar -czf ...` |
| BLACKSTAR build: `UNKNOWN error opening node_modules/sigma/package.json` | Windows filesystem race on freshly added dep | `rm -rf node_modules && pnpm install` on BLACKSTAR |
| Windows: Vite 8 build fails / version error | system node < 22.12 | `nvm install 22.22.0 && nvm use 22.22.0` on builder |
| macOS CI: `codesign ... no identity found` for bundled CLI/sidecar | workflow passed a stale `APPLE_SIGNING_IDENTITY` secret that does not match the imported p12 certificate | after `apple-actions/import-codesign-certs`, resolve `Developer ID Application` with `security find-identity -v -p codesigning` and export that to `GITHUB_ENV`; do not override it with the secret |
| release workflow mirror step → `EXTRA_FLAGS[@]: unbound variable` | empty Bash array expansion under `set -u` on older Bash | avoid optional-flag arrays in release shell; branch explicitly for prerelease vs stable `gh release create` |
| Windows: `sqlite-sys` cc-rs → "malloc.h not found" | `build.sh`'s `SDK_VER` (10.0.22621.0) ≠ installed Windows Kit | check `ls "/c/Program Files (x86)/Windows Kits/10/Include"` then `export SDK_VER=<found-version>` (e.g. 10.0.26100.0 on ROG) |
| `gh release upload ...` → "no matches found" on Windows | MSYS git-bash + stale cd + relative paths | `cd` back to repo root before upload, or use absolute paths |
| `git rev-parse v$VER` returns a SHA that doesn't appear in `git log` | annotated tag (`-a`) creates a tag object that points to the commit (one level of indirection) | use `git rev-parse v$VER^{commit}` for the commit SHA, or `gh release view v$VER --json targetCommitish` |
| `codeterm send-text --mesh PEER --pane ID "..."` does not get a reply | target pane is busy or the receiving agent has not processed the input yet | send-text is fire-and-forget by design; check output later with `codeterm get-output --mesh PEER --pane ID` |
| SSH to BLACKSTAR: `cd /c/…` fails | `bash -lc` runs a login shell that re-homes | use `bash -c` (non-login) |
| `scp User@blackstar:/c/…` fails | remote shell is cmd.exe, doesn't grok the path | stage the file to `C:\Users\dev\` on BLACKSTAR first, then `scp User@blackstar:filename` |
| Public `curl` of release asset → 404 | repo private, CDN rejects unauthenticated | use `gh release download`, or make repo public |
| Tag pushed but version files still say OLD | step 1 sed didn't catch all 3 (quote style, OLD value wrong) | run the post-bump verification grep BEFORE pushing the tag |
| `codeterm update check` returns stale `Latest:` after a fresh release | daemon caches the manifest at startup; default TTL outlasts your release | `codeterm update check --force` bypasses cache. GUI path (tauri-plugin-updater via public mirror endpoint) is independent of daemon cache — that always sees fresh |

## Multi-machine operational protocol

Adopted v1.1.8 onward. Cross-machine release coordination between Mac-Air (macOS signing host), ROG / BLACKSTAR (Windows builders), and the rest of the mesh.

**Roles per release:**
- **macOS builder (Mac-Air)**: macOS arm64 + x86_64 builds, notarize+staple, minisign all 3 platforms, compose `latest.json`, publish public mirror.
- **Windows builder (ROG primary, BLACKSTAR fallback)**: Windows build, `.exe` + `.nsis.zip` upload to private repo draft.
- **Either side** can make the final "mark Latest on private" call after public mirror is verified live.

**Communication contract:**
- Mesh messages limited to milestones and blockers — not ad-hoc "where are you" polls.
- Default for status checks: fetch from `gh` + `git` (release asset list, master HEAD, tag SHA) without pinging the other side. Ping only when those don't tell the story, or at a milestone gate.
- Milestone format: `v$VER milestone: $stage — $details`. Stages: `tagged`, `win-built`, `mac-built`, `signed`, `public-live`, `private-published`.
- Blocker format: `v$VER BLOCKER: $where — $reason`. Reply within ~5 min or the other side may proceed unilaterally on small calls.

**Unilateral permissions (no ack needed each time):**
- Either side: cfg-only patches to fix per-target build breaks, force-retag tags pre-publish, regenerate `latest.json`.

**Catastrophic-only escalations (always ack required from the other side):**
- master force-push that touches commits already in BOTH local copies.
- minisign or Apple Developer cert key rotation.
- public mirror destructive ops (delete release, change `tauri.conf.json` updater endpoint URL, alter pubkey).

**Tiebreaker:** Andrey (local user) decides anything contested.

**Why:** prior coordination ping-ponged every blocker which stalled both sides. Codifying default permissions and milestone-only comms reduces interrupts. Catastrophic-ack list contains ops that can lose work or brick existing installs.

## Cross-machine signing when Bitwarden is locked

If the Windows builder can't access the minisign key (Bitwarden locked, no `~/.tauri/codeterm.key` on disk), delegate signing to whichever peer DOES have BW unlocked. Used 2026-05-13: ROG built `.nsis.zip` but BW was locked there; BLACKSTAR did the signing.

```bash
# On the build host (ROG), send the file to the signer peer via Taildrop:
tailscale file cp ~/Downloads/CodeTerm_${VER}_x64-setup.nsis.zip BLACKSTAR:

# On the signer host (BLACKSTAR):
# 1. File lands in ~/Downloads via the Windows GUI Tailscale (NOT the CLI inbox —
#    `tailscale file get` returns 0/0 because GUI Tailscale handles incoming differently).
#    May need to `mv ~/Downloads/CodeTerm_*.nsis.zip ./` manually.

# 2. Verify hash matches what the builder reported:
sha256sum CodeTerm_${VER}_x64-setup.nsis.zip
# → compare against builder's number

# 3. Pull minisign key from Bitwarden (BW unlocked here):
codeterm mem secret get --name codeterm-tauri-minisign-private-key > ~/.tauri/codeterm.key

# 4. Sign — key is used AS-IS (single-line base64), don't base64-decode:
KEY=$(cat ~/.tauri/codeterm.key | tr -d '\n')
TAURI_SIGNING_PRIVATE_KEY=$KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD='' \
  pnpm --package=@tauri-apps/cli dlx tauri signer sign CodeTerm_${VER}_x64-setup.nsis.zip

# 5. Upload the .sig to the private repo:
gh release upload v$VER CodeTerm_${VER}_x64-setup.nsis.zip.sig --clobber --repo rollacode/codeterm
```

**Important:** if Mac-Air already published the public mirror with its own valid sig (signs everything from one place), the cross-machine `.sig` upload to the private repo is redundant but harmless. The public mirror's `latest.json` is what the auto-updater actually consumes.

## Local install paths — pre-release dev/test only

Three ways to get a freshly-built binary running on YOUR machine, in increasing order of "real" install:

### Path 1 — manual file swap (Windows, debug-only)

Use when iterating on a prod-mode-only bug and you need fresh `codeterm.exe` in the running install without touching the installer:

```bash
# 1. Kill running prod (port 7685)
powershell.exe -NoProfile -Command "Get-Process codeterm -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Seconds 2"

# 2. Copy fresh binaries into the installed location
cp C:/Developer/codeterm/src-tauri/target/release/codeterm.exe \
   "$USERPROFILE/AppData/Local/CodeTerm/codeterm.exe"
cp C:/Developer/codeterm/src-tauri/target/release/codeterm-embed.exe \
   "$USERPROFILE/AppData/Local/CodeTerm/codeterm-embed.exe"   # only if you touched the sidecar

# 3. Restart
powershell.exe -NoProfile -Command "Start-Process -FilePath 'C:\Users\dev\AppData\Local\CodeTerm\codeterm.exe'"

# 4. Verify
curl -s http://127.0.0.1:7685/api/health
```

If the window shows `ERR_CONNECTION_REFUSED` for localhost → your build was compiled in dev mode (devUrl = http://localhost:1420). Re-run `./build.sh build`, not raw `cargo build --release`.

Known gotcha — WIX MSI bundler may fail on Windows (`failed to run light.exe`) but the `.exe`s are already produced by then; NSIS / file-swap paths still work.

### Path 2 — self-install via CLI (Windows, programmatic)

`codeterm update install --from-file` runs the freshly-built NSIS installer silently in a detached helper, swaps the binary, and restarts:

```bash
codeterm update install --from-file 'C:\Developer\codeterm\src-tauri\target\release\bundle\nsis\CodeTerm_X.Y.Z_x64-setup.exe'

# With session continuation (resume the calling agent's session after restart):
codeterm update install \
  --from-file '...\CodeTerm_X.Y.Z_x64-setup.exe' \
  --continue-prompt "after restart, verify tab performance"
```

`--continue-pane` defaults to `$CODETERM_PANE_ID`. The helper:
1. Sleeps 2s, lets the calling CLI return.
2. `taskkill /F /IM codeterm.exe codeterm-embed.exe`.
3. Runs the NSIS installer with `/S` (silent — no UAC since `installMode=currentUser`).
4. Launches the freshly installed `codeterm.exe`.
5. If `--continue-prompt`: polls `/api/health` until 200, then `codeterm send-text --pane <id>` resumes the calling agent.
6. Logs to `%LocalAppData%\codeterm\self-install.log`.

Fallback when CLI itself is broken or version-mismatched — use the standalone PowerShell script:
```powershell
powershell -ExecutionPolicy Bypass `
  -File "$env:USERPROFILE\.codeterm\skills\self-install-local\scripts\self-install-local.ps1" `
  -Installer "C:\Developer\codeterm\src-tauri\target\release\bundle\nsis\CodeTerm_X.Y.Z_x64-setup.exe" `
  -Resume
```

The script lives in this skill's folder at `<repo>/.codeterm/skills/github-release/scripts/self-install-local.ps1`.

Gotchas:
- Don't pass `-Verb RunAs` to the installer. `installMode=currentUser` doesn't need elevation; RunAs triggers UAC and hangs on headless.
- Helper survives the daemon, not your terminal pane — if invoked from a CodeTerm-managed pane, the pane dies with the daemon. Tail progress from outside: `Get-Content -Wait "$env:LocalAppData\codeterm\self-install.log"`.
- Auto-resume of agent panes is best-effort — `~/.codeterm/sessions/last.json` may be slightly behind if the daemon was killed abruptly.

### Path 3 — full release publish

The rest of this skill. Use when shipping to users. The local-install paths above are NOT a substitute; the public `latest.json` must be updated for end-users' auto-update to work.

## Why no specialized "devops" agent

As of 2026-05-12 this skill is the canonical release procedure. No `.claude/agents/devops.md` exists. When the user says «релиз», «выпусти vX.Y.Z», «ship it», any Lead or Sonnet worker can execute by fetching this skill from mem (`codeterm mem get skill_end_to_end_github_release_for_codeterm`) and following it end-to-end. Single source of truth, no drift between `.md` and `mem`.

## Known gaps (fix opportunistically)

- `./build.sh build` should staple the DMG wrapper automatically — currently manual step in §5. Worth patching `build.sh` to mirror what `scripts/release-macos-x86_64.sh` already does.
- No CI release workflow — everything's local. A GitHub Action on tag push would replace §§5-8.
- Brew tap bump is manual — a `scripts/release-bump-brew.sh` could read the release JSON and rewrite the cask in one shot.
- `tailscale file get` on Windows ignores GUI-Tailscale-received files (they land in `~/Downloads`, not the CLI inbox). Worth verifying / filing upstream.
