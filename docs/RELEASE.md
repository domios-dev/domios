# Releasing CodeTerm

How a versioned release goes from `develop` to a published GitHub release the
in-app updater can pull. This is the **map**; the **executable procedure** (every
command, every secret, every retry) lives in the `github-release` skill:
`.codeterm/skills/github-release/SKILL.md`. For a local build with no release, use
the `prod-build` skill instead.

**Golden rule:** every artifact is built from the **tagged commit on `master`** —
never from `develop`, never from an untagged tree.

## Two-repo layout

| Repo | Role |
|---|---|
| `rollacode/codeterm` | private source. Releases here are archival only — the GUI updater can't read private-repo assets anonymously. |
| `rollacode/codeterm-releases` | public mirror: artifacts + signed `latest.json`. `tauri.conf.json`'s updater endpoint points here. **Without the mirror, in-app `update install --auto` 404s.** |

## The flow

1. **Bump version on `develop`** (one commit). Run
   `npm run sync:versions -- X.Y.Z`, then `./build.sh check` to refresh
   `Cargo.lock`, then `npm run check:versions`. The script is the single version
   sync point for root package metadata, Tauri config, Rust packages, provider
   plugins, `plugins/channel.json`, and shared packages. Commit `release:
   vX.Y.Z`, push.
2. **Merge `develop` → `master`** via a GitHub PR (merge commit), then locally
   `git checkout master && git pull && git tag -a vX.Y.Z -m "…" && git push --tags`.
3. **Draft the GitHub release** (`gh release create vX.Y.Z --draft`).
4. **Checkout the tag** (`git checkout vX.Y.Z` → detached HEAD). All builds run
   from this tree.
5. **CI builds on tag push**: `release-macos`, `release-windows`, and
   `release-linux` build from the tag, upload private source artifacts, produce
   updater fragments whose URLs point at `rollacode/codeterm-releases`, and
   mirror public assets + merged `latest.json`.
6. **Manual builder fallback**: if a platform workflow is down, use the skill's
   Mac/Windows mesh instructions. If the source artifacts exist but mirror upload
   failed, use the skill's manual mirror fallback instead of rebuilding.
7. **Publish** the release (un-draft) and verify the updater endpoint serves the
   new `latest.json`.
8. **Sync `master` → `develop`** and return to `develop`.
9. (Optional, while repos are private) bump the Homebrew cask.

## Gotchas that have cost real build time

- **Windows updater artifact is the NSIS `.exe` in Tauri v2 mode.** This repo
  uses `bundle.createUpdaterArtifacts = true`; `.nsis.zip` belongs to the old
  `"v1Compatible"` mode. Do not make the manifest point at `.nsis.zip` unless
  the config is intentionally changed.
- **macOS notarize** needs a *real* `Developer ID Application` signature with the
  hardened-runtime flag (`flags=0x10000`) — `build-mac.sh` sanity-checks this
  before submitting, so you don't discover an ad-hoc signature 30 min into
  Apple's queue.
- **macOS updater tar** must use `tar --no-mac-metadata`, else AppleDouble
  `._CodeTerm.app` shadows make the updater reject the bundle.
- **Sign with `node_modules/.bin/tauri signer sign`** on Windows — `npx … signer
  sign` fails with *"Unknown command"*.
- **Verify version parity before tagging** (step 1) — the cheapest place to catch
  a mismatch; the most expensive is a finished build with the wrong filename.
- **macOS Intel CI must use `macos-15-intel`, not `macos-13`.** GitHub retired
  `macos-13` in December 2025; x86 jobs on that label can sit queued forever.
- **`PACKAGE_REPOS_TOKEN` must have `contents:write` on
  `rollacode/codeterm-releases`.** The source release can upload with
  `GITHUB_TOKEN`, but the public updater mirror is a different repo and fails
  with HTTP 403 when this token is missing or scoped only to the source repo.
- **macOS updater URLs must be arch-specific.** Never point both
  `darwin-aarch64` and `darwin-x86_64` at generic `CodeTerm.app.tar.gz`; the
  second macOS job can clobber that asset. Use `CodeTerm_aarch64.app.tar.gz`
  and `CodeTerm_x64.app.tar.gz`.
- **`latest.json` URLs must point at `rollacode/codeterm-releases`, not the
  private source repo.** The updater runs anonymously, so private source URLs
  fail even when the public mirror exists.
- **Tauri v2 updater artifact names follow `createUpdaterArtifacts`.** With our
  current `true` setting, Linux uses `.AppImage` and Windows uses NSIS `.exe`.
  `.AppImage.tar.gz` / `.nsis.zip` are for `"v1Compatible"` only.
- **If source release assets are valid but mirror failed**, download from
  `rollacode/codeterm`, regenerate `latest.json` with public
  `rollacode/codeterm-releases` URLs, upload to the mirror with `--clobber`, and
  only then publish the private source release.

## Secrets (per builder machine)

The minisign updater key (`~/.tauri/codeterm.key`, empty password; its public
half is baked into `tauri.conf.json::plugins.updater.pubkey`) and the Apple
notary credentials are backed up in mem — the skill lists the exact ids and
restore paths. A builder without the minisign key produces artifacts the updater
can't verify.

<!-- revision: 7 -->
