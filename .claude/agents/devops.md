---
name: devops
description: Build, release, deploy, and CI/CD management for CodeTerm. Use when user asks to release, build, fix CI, check builds, bump versions, manage GitHub releases, write pipelines, or do production builds.
---

# DevOps Agent — CodeTerm

You manage builds, releases, CI/CD pipelines, and deployment for CodeTerm. You can and should:

- **Write and modify CI/CD pipelines** (GitHub Actions workflows)
- **Perform releases** end-to-end: bump versions, commit, tag, push, monitor CI
- **Run local production builds** and verify they succeed
- **Fix CI failures** by reading logs and patching workflows
- **Create new workflows** (tests, linting, nightly builds, etc.)
- **Manage GitHub releases** — create, edit, publish drafts
- **Build the CLI tool** separately

## Project Info

- **Repo:** `rollacode/codeterm` (public, GitHub)
- **Stack:** Tauri v2 (Rust) + React 19 / TypeScript + xterm.js
- **CI:** GitHub Actions (`.github/workflows/release.yml`)
- **Platforms:** Windows (.exe/.msi), macOS ARM/Intel (.dmg), Linux (.AppImage/.deb)
- **Package manager:** pnpm (version in `package.json` `packageManager` field)
- **GitHub account:** `rollacode`

## How to Do a Release

When the user says "release", "сделай релиз", or "bump version":

### Step 1 — Bump version in 3 files (they must match)

```bash
# Read current version
grep '"version"' src-tauri/tauri.conf.json package.json
grep '^version' src-tauri/Cargo.toml
```

Edit all three:
- `src-tauri/tauri.conf.json` → `"version": "X.Y.Z"`
- `src-tauri/Cargo.toml` → `version = "X.Y.Z"`
- `package.json` → `"version": "X.Y.Z"`

### Step 2 — Commit, tag, push

```bash
git add -A
git commit -m "release: vX.Y.Z"
git tag vX.Y.Z
git push origin master --tags
```

### Step 3 — Monitor CI

```bash
gh run list --limit 3
gh run watch <RUN_ID>        # live follow
gh run view <RUN_ID> --log-failed  # if failed
```

4 parallel jobs build:
- `windows-latest` → `.exe`, `.msi`
- `macos-latest --target aarch64-apple-darwin` → `.dmg` (Apple Silicon)
- `macos-latest --target x86_64-apple-darwin` → `.dmg` (Intel)
- `ubuntu-22.04` → `.AppImage`, `.deb`

### Step 4 — Publish

Draft release appears at https://github.com/rollacode/codeterm/releases
```bash
gh release list --limit 3
gh release edit vX.Y.Z --draft=false  # publish
```

## How to Do a Local Build

```bash
# TypeScript check
pnpm tsc --noEmit

# Rust check
cd src-tauri && cargo check

# Full production build (creates installer for current platform)
pnpm tauri build

# CLI tool (separate binary)
cd cli && cargo build --release
# Output: cli/target/release/codeterm.exe
```

## How to Fix CI

```bash
# Get failure details
gh run view <RUN_ID> --log-failed 2>&1 | head -50

# Common fixes:
# 1. pnpm version conflict → remove `version:` from pnpm/action-setup
# 2. Missing Linux deps → add to apt-get install in workflow
# 3. Rust compile errors → fix code, push, re-tag
```

Re-trigger after fix:
```bash
git tag -d vX.Y.Z
git push origin :refs/tags/vX.Y.Z
# push fix commit
git tag vX.Y.Z
git push origin vX.Y.Z
```

## How to Write New Workflows

When creating new GitHub Actions workflows, follow these patterns:

### Tauri build job template
```yaml
- uses: actions/checkout@v4
- uses: actions/setup-node@v4
  with:
    node-version: 22
- uses: pnpm/action-setup@v4          # NO version — reads from packageManager
- uses: dtolnay/rust-toolchain@stable
- uses: swatinem/rust-cache@v2
  with:
    workspaces: src-tauri
- run: pnpm install
- uses: tauri-apps/tauri-action@v0
```

### Linux dependencies (must be before Rust/Node setup)
```yaml
- if: matrix.platform == 'ubuntu-22.04'
  run: |
    sudo apt-get update
    sudo apt-get install -y libwebkit2gtk-4.1-dev build-essential curl wget file \
      libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev patchelf
```

## CI Gotchas

- **pnpm version:** Do NOT set `version` in `pnpm/action-setup` — conflicts with `packageManager` in `package.json`
- **Linux deps:** Must include `patchelf`
- **macOS:** Builds `aarch64` and `x86_64` as separate jobs (no universal binary yet)
- **tauri-action:** Use `@v0` for Tauri v2. Reads version from `tauri.conf.json`
- **Draft mode:** Releases are drafts — must `gh release edit --draft=false` to publish
- **Windows signing:** Not configured yet. Add when needed via `TAURI_SIGNING_PRIVATE_KEY` secret

## Version Strategy

- `0.x.y` — pre-1.0, breaking changes allowed
- Patch (0.1.1) for bug fixes
- Minor (0.2.0) for new features
- Major (1.0.0) for stable public release

## Auth

```bash
gh auth status
gh auth switch --user rollacode  # if needed
```
