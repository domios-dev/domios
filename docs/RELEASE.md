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
| `rollacode/codeterm-releases` | public mirror: artifacts + signed `latest.json`. `tauri.conf.json`'s updater endpoint points here. **Without the mirror, in-app `update --apply` 404s.** |

## The flow

1. **Bump version on `develop`** (one commit). Three files must agree:
   `src-tauri/Cargo.toml` (`version = "X.Y.Z"`), `src-tauri/tauri.conf.json`,
   `package.json`. Run `./build.sh check` to refresh `Cargo.lock`, then `grep`
   all three — a missed `sed` isn't caught until a builder bakes the stale
   version into the installer filename. Commit `release: vX.Y.Z`, push.
2. **Merge `develop` → `master`** via a GitHub PR (merge commit), then locally
   `git checkout master && git pull && git tag -a vX.Y.Z -m "…" && git push --tags`.
3. **Draft the GitHub release** (`gh release create vX.Y.Z --draft`).
4. **Checkout the tag** (`git checkout vX.Y.Z` → detached HEAD). All builds run
   from this tree.
5. **macOS** (on a Mac): build → codesign (`Developer ID Application`) →
   notarize+staple → pack+minisign the updater bundle, for **both** arm64 and
   x86_64. Upload DMGs to the private repo; sign them, build `latest.json`, and
   mirror artifacts + manifest to the public repo.
6. **Windows** (on a paired mesh peer — **BLACKSTAR** is primary): dispatch
   `scripts/win-release.sh vX.Y.Z`. It checks out the tag, installs, bundles the
   NSIS `.exe` + `.nsis.zip`, signs, and uploads to both repos. The agent's final
   `codeterm agent report` line is machine-parseable: `WINDOWS_READY=X.Y.Z sig=…`
   on success, `BLOCKED step=…` on failure.
7. **Publish** the release (un-draft) and verify the updater endpoint serves the
   new `latest.json`.
8. **Sync `master` → `develop`** and return to `develop`.
9. (Optional, while repos are private) bump the Homebrew cask.

## Gotchas that have cost real build time

- **Windows updater bundle must be STORED-compressed, not DEFLATE.** Export
  `TAURI_SIGNING_PRIVATE_KEY` (+ `_PASSWORD`) **before** `./build.sh build`, or
  the bundler skips the `.nsis.zip` and the fallback ships a DEFLATE zip that
  `tauri-updater` rejects with *"Compression method not supported"*. Verify bytes
  8–9 of the local file header are `00 00`: `od -A x -t x1 -N 10 *_x64-setup.nsis.zip`.
  (`08 00` = DEFLATE = broken.) `win-bundle.sh` checks this and exits `BLOCKED:`.
- **Windows zip fallback** must use `python zipfile.ZIP_STORED` — never
  `Compress-Archive` (always DEFLATE).
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

## Secrets (per builder machine)

The minisign updater key (`~/.tauri/codeterm.key`, empty password; its public
half is baked into `tauri.conf.json::plugins.updater.pubkey`) and the Apple
notary credentials are backed up in mem — the skill lists the exact ids and
restore paths. A builder without the minisign key produces artifacts the updater
can't verify.

<!-- revision: 1 -->
