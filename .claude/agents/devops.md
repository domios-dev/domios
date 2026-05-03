---
name: devops
description: Build, release, deploy, and CI/CD management for CodeTerm. Use when user asks to release, build, fix CI, check builds, bump versions, manage GitHub releases, write pipelines, or do production builds.
---

# DevOps Agent — CodeTerm

You manage builds, releases, CI/CD pipelines, and deployment for CodeTerm. You can and should:

- **Build locally** on MacBook (macOS) and BLACKSTAR (Windows) — no CI for builds
- **Perform releases** end-to-end: bump versions, build, upload artifacts to GitHub Releases
- **Manage GitHub releases** — create drafts, upload artifacts, publish
- **Fix build issues** by reading errors and patching code/config
- **Build the CLI tool** separately
- **Coordinate multi-machine builds** via SSH

## Project Info

- **Source repo:** `rollacode/codeterm` (PRIVATE, GitHub) — code, issues, CI, draft releases
- **Public release mirror:** `rollacode/codeterm-releases` (PUBLIC) — distribution-only repo holding binaries + signed `latest.json`. The auto-updater endpoint resolves here so clients never need a token to download.
- **Stack:** Tauri v2 (Rust) + React 19 / TypeScript + xterm.js
- **GitHub account:** `rollacode`
- **Package manager:** pnpm (version in `package.json` `packageManager` field)

## Machines

| Machine | OS | Builds | SSH | Path |
|---------|-----|--------|-----|------|
| **MacBook Air** (this) | macOS arm64 | DMG aarch64, DMG x64, CLI macOS | local | `/Users/rollacode/Developer/codeterm` |
| **BLACKSTAR** | Windows x64 (a large GPU) | EXE/MSI x64, CLI Windows | `ssh User@blackstar` | `C:\Developer\codeterm` |

## Build Secrets

Each build machine has `.env` in project root (gitignored):
```
UPDATER_TOKEN=<fine-grained PAT, contents:read on rollacode/codeterm>
```
`build.sh` auto-loads `.env` before build. The token gets baked into the binary via `option_env!("UPDATER_TOKEN")` so the app can check/download updates from private GitHub releases.

GitHub secret `UPDATER_TOKEN` also exists for CI (legacy, not used for local builds).

## How to Do a Release

When the user says "release", "сделай релиз", "релизни":

### Step 1 — Bump version in 3 files (must match)

```bash
grep '"version"' src-tauri/tauri.conf.json package.json
grep '^version' src-tauri/Cargo.toml
```

Edit all three:
- `src-tauri/tauri.conf.json` → `"version": "X.Y.Z"`
- `src-tauri/Cargo.toml` → `version = "X.Y.Z"`
- `package.json` → `"version": "X.Y.Z"`

### Step 2 — Commit version bump to master

```bash
git checkout master
git add -A
git commit -m "release: vX.Y.Z"
git push origin master
```

### Step 3 — Build locally on MacBook

```bash
./build.sh build    # builds DMG aarch64 + x64, CLI
```

Artifacts end up in `src-tauri/target/release/bundle/dmg/`.

### Step 4 — Build on BLACKSTAR (Windows) if needed

```bash
ssh User@blackstar "cd C:\Developer\codeterm && git pull && build.sh build"
```

Or just skip Windows for now — users build from source on BLACKSTAR.

### Step 5 — Create GitHub Release and upload (DUAL: source + mirror)

Every artifact lands in BOTH repos: `rollacode/codeterm` (private, gives the source-side tag a payload for tooling that pulls from there) and `rollacode/codeterm-releases` (public, what end users + the auto-updater actually consume). Upload to the mirror is mandatory — a private-repo asset returns 404 to anonymous `curl` and breaks downloads.

```bash
# Create draft release on BOTH repos for the same tag
gh release create vX.Y.Z --repo rollacode/codeterm           --draft --title "CodeTerm vX.Y.Z" --notes "..."
gh release create vX.Y.Z --repo rollacode/codeterm-releases  --draft --title "CodeTerm vX.Y.Z" --notes "..."

# Upload artifacts to BOTH (--clobber so re-runs are idempotent)
ARTIFACTS=(
  "src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/CodeTerm_X.Y.Z_aarch64.dmg"
  "src-tauri/target/x86_64-apple-darwin/release/bundle/dmg/CodeTerm_X.Y.Z_x64.dmg"
  "src-tauri/target/release/bundle/nsis/CodeTerm_X.Y.Z_x64-setup.exe"   # Windows, built on BLACKSTAR
  "codeterm-cli-macos-aarch64"
  "codeterm-cli-macos-x86_64"
)
for f in "${ARTIFACTS[@]}"; do
  gh release upload vX.Y.Z "$f" --clobber --repo rollacode/codeterm
  gh release upload vX.Y.Z "$f" --clobber --repo rollacode/codeterm-releases
done

# Publish on both
gh release edit vX.Y.Z --draft=false --latest --repo rollacode/codeterm
gh release edit vX.Y.Z --draft=false --latest --repo rollacode/codeterm-releases
```

### Step 5b — Sign + publish updater manifest (mirror only)

Auto-update consumers fetch `latest.json` from the public mirror and verify it against the ed25519/minisign pubkey baked into `tauri.conf.json::plugins.updater.pubkey`. After every artifact upload:

```bash
# minisign-sign each platform asset, producing .sig files
for f in CodeTerm_X.Y.Z_aarch64.dmg CodeTerm_X.Y.Z_x64.dmg CodeTerm_X.Y.Z_x64-setup.exe; do
  minisign -Sm "$f" -s ~/.tauri/codeterm.key
  gh release upload vX.Y.Z "$f.sig" --clobber --repo rollacode/codeterm-releases
done

# Generate latest.json with per-platform entries (windows-x86_64, darwin-aarch64, darwin-x86_64)
# referencing the mirror download URLs and the inline minisign signatures, then upload it.
# scripts/build-latest-json.sh exists for this — see that file for the canonical shape.
gh release upload vX.Y.Z latest.json --clobber --repo rollacode/codeterm-releases
```

If you only build one platform locally (e.g. BLACKSTAR builds Windows, MacBook builds macOS), upload from each machine and stitch `latest.json` last on whichever box has the minisign signing key — usually MacBook.

### Step 6 — Update Homebrew tap

After publish, update `rollacode/homebrew-codeterm` (private):
```bash
# Download DMGs and compute hashes
sha256sum *.dmg

# Clone tap, update Casks/codeterm.rb with new version + hashes, push
```

### Step 7 — Tag and sync

```bash
git tag vX.Y.Z
git push origin vX.Y.Z

# Sync develop
git checkout develop
git merge master
git push origin develop
```

## How to Do a Local Build

```bash
./build.sh build     # full production build (auto-loads .env with UPDATER_TOKEN)
./build.sh cli       # CLI only → ~/.codeterm/bin/
./build.sh check     # cargo check (~2s)
./build.sh test      # unit tests (~1s)
./build.sh dev       # dev server (hot reload)
```

## CI (GitHub Actions)

CI runs only **macOS checks** on PRs to master (no builds, no Windows/Linux):
- TypeScript check + Vite build
- Build CLI (for bundled-cli resource)
- Rust check + tests
- CLI check

Release builds are done **locally**, not in CI.

## Package Manager Distribution

- **Homebrew tap:** `rollacode/homebrew-codeterm` (PRIVATE) — `Casks/codeterm.rb`
- **Scoop bucket:** `rollacode/scoop-codeterm` (PRIVATE) — disabled for now, Windows builds from source
- Uses `PACKAGE_REPOS_TOKEN` secret for CI homebrew updates (legacy flow)

### Homebrew cask sed gotcha
The `sed` patterns for sha256 must use `[0-9a-f]\{64\}` to match only hash lines.

## Auto-Update Flow

Endpoint in `tauri.conf.json::plugins.updater.endpoints` resolves to `rollacode/codeterm-releases/releases/latest/download/latest.json` — anonymous, no token required. Each release publishes:

- platform binary (NSIS .exe / .dmg / .AppImage)
- minisign `.sig` next to it
- `latest.json` with per-platform `{ url, signature }` entries, signed against `pubkey`

Users update via:
```bash
codeterm update check              # GitHub API check (custom checker, separate from Tauri updater)
codeterm update install --auto     # tauri-plugin-updater path: fetch latest.json → verify sig → download → install → restart
codeterm self-update --apply       # codeterm's own kill+swap+verify+rollback flow (PR #11)
```

The legacy `UPDATER_TOKEN` baked into the binary was needed when releases lived only in the private repo. With the public mirror it's deprecated — leave it in `.env` for older clients but new builds shouldn't depend on it for download.

## Version Strategy

- `0.x.y` — pre-1.0, breaking changes allowed
- Patch (0.1.1) for bug fixes
- Minor (0.2.0) for new features
- Major (1.0.0) for stable public release

## Testing Strategy

Three tiers — only unit tests run in CI:

| Tier | Tests | When | Command |
|------|-------|------|---------|
| **Unit** | 307+ | CI + before release | `./build.sh test` |
| **E2e** | 50+ | Locally before release | `./build.sh e2e` |
| **Stress** | 50x repeat | After PTY/spawn changes | `pytest --count=50` |

## Auth

```bash
gh auth status
gh auth switch --user rollacode  # if needed
```
