# Releasing CodeTerm

How a versioned release goes from `develop` to a published GitHub release the
in-app updater can pull. This is the **map**; the **executable procedure** (every
command, every secret, every retry) lives in the `github-release` skill:
`.agents/skills/github-release/SKILL.md`. Build-farm/runner internals live in
`.codeterm/build-machine/README.md`. For a local build with no release, use the
`prod-build` skill instead.

**Golden rule:** every artifact is built from the **tagged commit on `master`** —
never from `develop`, never from an untagged tree.

This is the public-release operation only. For a local production installer use
`docs/BUILD.md#local-production-build-and-self-install`; for a dev window use
the `dev-run` skill. Do not substitute either for this workflow.

When a new release failure is discovered, repair the owning workflow or script
and add a short symptom/cause/recovery entry to this document or the release
skill before retrying. A successful rerun without updating the procedure is not
complete remediation.

## Two-repo layout

| Repo | Role |
|---|---|
| `rollacode/codeterm` | private source. Releases here are archival only — the GUI updater can't read private-repo assets anonymously. |
| `rollacode/codeterm-releases` | public mirror: artifacts + signed `latest.json`. `tauri.conf.json`'s updater endpoint points here. **Without the mirror, in-app `update install --auto` 404s.** |

## Build farm

Three self-hosted runners, one `release-X.Y.Z` tag fires all three in parallel:

| Runner | Machine | Workflow | Builds |
|---|---|---|---|
| `codeterm-builder` | Mac (ARM64) | `.github/workflows/release.yml` | macOS `x64`+`arm64` (Developer ID + notarized), **and** `arm64` Linux `.deb`+`.AppImage` (Docker) |
| `windows-build` | ROG (Windows x86_64) | `release-windows.yml` | Windows `x64`+`arm64` NSIS, signed on the runner |
| `linux-build` | ROG's WSL2 Ubuntu 24.04 (x86_64) | `release-linux.yml` | `x86_64` Linux `.deb`+`.AppImage`, native |

No SSH between machines; each runner builds, signs, and (macOS/Linux-arm64) publishes/mirrors
itself. Full setup and hard-won constraints: `.codeterm/build-machine/README.md`.

## 1. Preflight (before bumping the version)

Run from the canonical checkout, on `develop`:

```bash
git fetch origin --prune
git checkout develop
git pull --ff-only origin develop
git merge-base --is-ancestor origin/master HEAD || {
  git merge --no-ff origin/master --no-edit
  # resolve conflicts, run the gates below, push develop before bumping the version
}
git status --short                  # must be clean
curl -s http://127.0.0.1:7686/api/health
```

**Cargo.lock is part of preflight, not an afterthought.** `npm run sync:versions` rewrites the
Rust package versions but does not by itself guarantee `src-tauri/Cargo.lock` is in sync — run
`./build.sh check` right after `sync:versions` (it refreshes the lockfile) and include
`src-tauri/Cargo.lock` in the version-bump commit. A stale lockfile passed a first tag push in the
1.7.2 release: the initial `release-1.7.2` push (PR #241) had its CI runs cancelled, a follow-up
commit (`fix(release): sync Tauri lockfile version`) landed via PR #243, and the tag had to be
re-cut from the new merge commit — a full wasted build cycle across all three runners. Verify with
`git diff --exit-code src-tauri/Cargo.lock` returning clean before opening the master PR.

```bash
VER=X.Y.Z
npm run sync:versions -- "$VER"
./build.sh check                      # refreshes Cargo.lock
./build.sh release-preflight           # refreshes and verifies both Cargo lockfiles
npm run check:versions
git add package.json src-tauri/Cargo.toml src-tauri/Cargo.lock \
  src-tauri/tauri.conf.json cli/Cargo.toml cli/Cargo.lock plugins packages scripts
git commit -m "release: v$VER"
git push origin develop
```

Then start dev from the main checkout and visually confirm the app, clean old lane worktrees
(`../codeterm-worktrees/`), and open the PR `develop` → `master`.

## 2. Cut the release: one trigger tag

Always via PR — never direct-push `master`. After the PR is merged:

```bash
git checkout master
git pull --ff-only origin master
git rev-parse --abbrev-ref HEAD   # must print master
INTENDED_MASTER_SHA=$(git rev-parse HEAD)
git ls-remote --tags origin "refs/tags/release-$VER" "refs/tags/v$VER"   # must be empty
test "$(git rev-parse origin/master)" = "$INTENDED_MASTER_SHA"

git tag "release-$VER" "$INTENDED_MASTER_SHA"
git push origin "refs/tags/release-$VER"   # full ref: the branch shares this name
```

Push **only** `release-X.Y.Z`. Do not also create/push `vX.Y.Z` — the three workflows stamp and
publish under `vX.Y.Z` themselves. Watch it: `gh run watch --repo rollacode/codeterm`.

## 3. Artifact matrix

What the pipeline is designed to produce per release, and where each comes from:

| Platform/arch | Asset(s) | Built on | Notes |
|---|---|---|---|
| macOS arm64 | `CodeTerm_<ver>_aarch64.dmg`, `CodeTerm_aarch64.app.tar.gz` | `codeterm-builder` (native) | Developer ID signed + notarized |
| macOS x86_64 | `CodeTerm_<ver>_x64.dmg`, `CodeTerm_x64.app.tar.gz` | `codeterm-builder` (cross) | needs `CODETERM_MONTEREY_LIBCXX_DIR`, see Troubleshooting |
| Linux arm64 | `CodeTerm_<ver>_arm64.deb`, `CodeTerm_<ver>_aarch64.AppImage` | `codeterm-builder` (Docker) | image `codeterm-build-linux-arm64` |
| Linux x86_64 | `CodeTerm_<ver>_amd64.deb`, `CodeTerm_<ver>_amd64.AppImage` | ROG WSL2 (native) | `linux-build` runner |
| Windows x64 | `CodeTerm_<ver>_x64-setup.exe` (+ `.sig`) | ROG (native) | `windows-build` runner |
| Windows arm64 | `CodeTerm_<ver>_arm64-setup.exe` (+ `.sig`) | ROG (cross, `vcvarsall x64_arm64`) | same runner — **no separate machine or fallback path exists for this arch** |

Public mirror also carries `latest.json` (all four `platforms` keys: `darwin-aarch64`,
`darwin-x86_64`, `linux-x86_64`, `windows-x86_64`) plus per-arch `updater-*.json` fragments and a
Debian-specific `latest-deb.json`.

**Do not claim an artifact exists without checking the actual release asset list first** —
`gh release view vX.Y.Z --repo rollacode/codeterm-releases --json assets` — or the `latest.json`
`platforms` map. A platform missing from both means it was not shipped for that version, not that
the doc is out of date.

## 4. Public mirror + latest.json verification

```bash
.agents/skills/github-release/scripts/release-status.sh "$VER"   # one-shot: assets + manifest coverage + CI state

curl -sIL https://github.com/rollacode/codeterm-releases/releases/latest/download/latest.json \
  | grep -E '^HTTP' | tail -1     # -> HTTP/2 200

# per-platform URL reachability + version match
python3 - <<'PY'
import json, urllib.request
m = json.load(urllib.request.urlopen(
    "https://github.com/rollacode/codeterm-releases/releases/latest/download/latest.json"))
print("version:", m["version"])
for platform, entry in m["platforms"].items():
    req = urllib.request.Request(entry["url"], method="HEAD")
    print(platform, urllib.request.urlopen(req).status, entry["url"])
PY
```

`python3` is not on every builder's PATH (confirm with `which python3` first) — fall back to
`curl -sIL <url> | grep '^HTTP'` per platform URL if it's missing.

## 5. Rerunning a failed workflow — without retagging

A failed or cancelled run at the **same commit** the tag already points at does not need a new
tag. Retagging (`git tag -f` + `--force` push) is only for moving the trigger to a **different**
commit, and is unilaterally allowed only **pre-publish** (no artifacts public yet for that
version). Once anything has published, cut a new patch version instead of moving the tag.

Safe, no-retag options, in order of preference:

1. **Rerun the same workflow run** (fixes a transient/runner issue, same SHA, same tag):
   ```bash
   gh run rerun <run-id> --failed --repo rollacode/codeterm
   ```
2. **Manually dispatch a single workflow against the existing tag** to validate a runner fix in
   isolation, without touching the stable channel:
   ```bash
   gh workflow run release-windows.yml --ref master -f tag=vX.Y.Z-rcN
   # an -rc suffix routes to latest-rc.json; the stable mirror step is skipped
   ```
3. **Manual builder fallback** (the skill's per-platform scripts, e.g. `win-release.sh`) when the
   runner itself is down and you need the release shippable now — see Troubleshooting below.

Only force-move the tag (`git tag -f "release-$VER" && git push origin "release-$VER" --force`)
when you deliberately need every platform to rebuild from a new commit and nothing has published
yet.

## 6. Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| PR CI fails during `pnpm install` with `ERR_PNPM_OUTDATED_LOCKFILE` | A workspace manifest changed without regenerating the root lockfile, so CI's implicit frozen-lockfile check rejects it before tests run | Run `pnpm install --no-frozen-lockfile`, commit `pnpm-lock.yaml`, then verify locally with `pnpm install --frozen-lockfile` before rerunning CI |
| Linux Rust PR job fails at `fmt` before compilation | A broad Rust rename/refactor was committed without applying both workspace formatters | Run `cargo fmt --manifest-path src-tauri/Cargo.toml` and `cargo fmt --manifest-path cli/Cargo.toml`, commit the mechanical formatting, then require both commands with `-- --check` before rerunning CI |
| TS PR job reaches Biome, then parses suspended Playwright files or a generated plugin bundle | The global Biome include set accidentally covers surfaces excluded from active gates | Keep `tests/playwright/**` and generated plugin UI bundles outside `biome.json`'s active include set; do not repair or run the suspended suite as a release side effect |
| Linux Rust PR job fails in clippy after local `build.ps1 check` passed | The build check compiles but does not run the stricter repository clippy gate | Run `bash scripts/clippy-gate.sh` on the release head and fix the reported owning code rather than adding a new lint allow |
| Windows Rust PR job fails the mesh announce no-op tests while an earlier broad local test appeared green | The cached workspace run did not rebuild `codeterm-mesh`, and the peer fixture used an old relay protocol so an address no-op still reported a metadata change | Initialize the fixture with `RELAY_PROTOCOL_VERSION`, then run `build.ps1 test-one mesh apply_announce_empty_list_is_noop`, `build.ps1 test-one mesh apply_announce_same_addrs_returns_no_change`, and the full mesh crate before updating the PR |
| macOS x86_64 leg queues far longer than the arm64/Docker legs, or `release-macos-x86_64.sh` exits with `FATAL: set CODETERM_MONTEREY_LIBCXX_DIR` | The Mac runner's environment lost `CODETERM_MONTEREY_LIBCXX_DIR` (must point at x86_64/universal LLVM `libc++.1.dylib` + `libc++abi.1.dylib`; the arm64 Homebrew LLVM libc++ cannot be used for the x86_64 updater — it's bundled so ONNX Runtime doesn't depend on newer system libc++ symbols missing on macOS 12 Monterey) | Restore the env var **in the runner service's persistent environment** (not just an interactive shell — a `launchd`/`svc.sh` restart drops shell-only exports), then rerun per §5. In the 1.7.2 run this cost ~85 minutes of queued-but-not-started time before the build itself ran normally. |
| Linux arm64 leg fails to start, or the Docker build errors on missing tools | `codeterm-build-linux-arm64` Docker image is missing or stale on the Mac runner | `cd .codeterm/build-machine/docker && docker build -t codeterm-build-linux-arm64 -f Dockerfile.linux-arm64 .`. Also confirm Docker Desktop memory is ~20 GiB — the `codeterm` crate needs ≳8 GiB to compile and an under-provisioned Docker OOM-kills `rustc`. |
| `release-windows.yml` jobs show `cancelled` for both `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc` shortly after the tag push, no `.exe` uploaded by CI | The `windows-build` (ROG) self-hosted runner was offline/unreachable when the tag fired | Dispatch a worker on the ROG mesh peer to run `bash .agents/skills/github-release/scripts/win-release.sh $VER` directly (checkout → build → sign → upload, machine-parseable report). This produces the **x64** installer only — see next row. Once the runner is confirmed back online, `gh run rerun` (§5) can still be used for future tags; it does not retroactively fill in the missed run for this version. |
| `latest.json` / release assets have no `windows-aarch64` entry for this version | The manual x64 fallback above has no ARM64 equivalent — cross-compiling `aarch64-pc-windows-msvc` needs the runner's `vcvarsall x64_arm64` toolchain, which only exists on the registered `windows-build` runner, not on an ad-hoc peer | Do not synthesize or claim a `windows-aarch64` asset for a release built this way. Either wait for the runner and rerun `release-windows.yml` before publishing, or ship without it and note the gap in the release notes; 1.7.2 shipped without it. |
| Windows x64 succeeds, then ARM64 immediately fails with `rustup.exe not found` | `Swatinem/rust-cache` can clean `%USERPROFILE%\.cargo\bin` in the x64 post-job hook before the queued ARM64 job starts | Keep a copied proxy set outside Cargo's cache path (for ROG: `%USERPROFILE%\tools\rustup-proxies`) first in the runner `.env` `PATH`; restore Cargo's normal proxies, restart only the runner listener, then use `gh run rerun <run-id> --failed`. v1.7.4 proved this repair on the same tag/SHA. |
| An archived release manifest downloads a later release or 404s after a newer version becomes GitHub Latest | The manifest used `/releases/latest/download/…` floating asset URLs | Generate all `latest.json` and updater fragment URLs with `/releases/download/vX.Y.Z/…`. Track this in issue #265; do not use an archived manifest as a stable download record until fixed. |
| Public issue validation fails with `python: command not found` | The self-hosted runner exposes Python as `python3` only | Keep release workflows on `python3`; rerun the failed PR check after the workflow fix is pushed. |
| `release.yml` preflight fails with `No Keychain password item found for profile: codeterm`, blaming expired Apple Developer agreements | The notary **keychain profile** vanished. `notarytool store-credentials` writes to the data-protection keychain, which only persists from a real interactive login session — from the runner (or any agent shell) it reports success and stores nothing, so the profile rots silently and cannot be re-provisioned automatically | Fixed in 1.7.5: preflight and `notarize-dmg.sh` now authenticate with `--apple-id/--team-id/--password`, taken from the `APPLE_ID`/`APPLE_TEAM_ID`/`APPLE_PASSWORD` repo secrets and falling back to `~/.claude/.secrets/apple-notary.env`. No keychain profile is consulted. If preflight still fails, the credentials themselves are bad — verify with `xcrun notarytool history --apple-id … --team-id … --password …` before suspecting Apple agreements, because the old error message pointed at the wrong cause. |
| `git push origin release-X.Y.Z` → `src refspec release-X.Y.Z matches more than one` | The release branch and the trigger tag share a name, so the short refspec is ambiguous | Push the tag by its full ref: `git push origin refs/tags/release-X.Y.Z`. |
| Apple notarization is broken and the whole Mac release job dies before Linux arm64 builds | Preflight aborted on the first failed check | Preflight now evaluates every check and exits once at the end, so the log shows the full picture. Note the Mac job still owns Linux arm64: `build-all.sh`'s `run_wave` keeps a failing engine from aborting the wave, but `publish-release.sh` deliberately bails when an artifact is missing, so a broken macOS leg still holds back the publish. To ship Linux arm64 independently the job would have to be split — not done yet. |
| A release bundle fails with missing `dist`, `bundled-cli`, or `bundled-embed` after `beforeBuildCommand` was removed | A direct Tauri release entrypoint bypassed `build.sh release-inputs`, so the frontend or target-correct sidecars were never staged | Keep `beforeBuildCommand` absent. Run the target-aware `release-inputs` operation before every direct Tauri release build, then rerun the unpublished release from the same verified source commit. |
| Windows release-input preparation builds the embed binary, then `bundle-embed.js` reports that `rustc` is not recognized | A nested Windows command shell cannot resolve bare `rustc` even though Cargo inherited a valid toolchain path | Use the current helper, which resolves the compiler from `RUSTC` or Cargo home without a shell lookup, then rerun `build.ps1 release-inputs`. |
| Windows x64 publishes but ARM64 fails with `LNK4272` and unresolved CRT symbols such as `memcpy` or `memset` | `vcvarsall x64_arm64` left x64 MSVC/SDK directories in `LIB`, so the ARM linker consumed host libraries | Use the current workflow, which replaces the cross-job library path with ARM64 MSVC/UCRT/UM directories and rejects any surviving x64/x86 path before compilation; rerun only the failed ARM64 job. |
| ARM64 fails at `release-inputs` with `LNK1120: 50 unresolved externals` (`__imp_malloc`, `_tls_index`, `__C_specific_handler`, `__imp__beginthreadex`) while the Tauri link itself is fine | `build.sh`'s `setup_env` hardcoded x64 `LIB`/`CC` and overwrote the workflow's ARM64 library path, so the CLI and `codeterm-embed` cross-builds it starts linked ARM64 objects against host CRT/SDK import libraries. The Tauri step escaped this because it runs outside `build.sh` | Use the current `setup_env`, which derives `LIB` and the target-scoped `CC_*`/`AR_*` from `CARGO_BUILD_TARGET` and aborts on any non-target library path. Verify with `bash scripts/check-msvc-cross-env.sh`, then rerun only the failed ARM64 job. |
| Publish succeeds but public-mirror verification rejects updater URLs under `/releases/latest/download/` | The mac/Linux-ARM fragment publisher emitted mutable channel aliases while the verifier requires immutable release assets | Regenerate the fragments with `/releases/download/vX.Y.Z/` URLs, merge all platform fragments again, replace `latest.json` in both releases, and verify every anonymous URL and signature. |
| macOS needs to be built while the self-hosted Mac is unavailable | — | `release-macos.yml` is the GitHub-hosted fallback: `gh workflow run release-macos.yml --ref master -f tag=vX.Y.Z`. It is **disabled by default** (hosted macOS minutes bill at 10×) — `gh workflow enable release-macos.yml` first and disable it again afterwards. It builds macOS only, never Linux arm64. Its checkout pins no `ref`, so it builds the default branch HEAD: verify `origin/master` still equals the release commit immediately before dispatching. |

## 7. Runner restoration boundaries

Safe for an agent to do unilaterally:
- Restart a runner service (`./svc.sh start`/`stop` on the Mac; the equivalent runner-service
  restart on ROG) and confirm it shows **Idle** under repo Settings → Actions → Runners.
- Re-export an env var whose correct value is already documented/backed up (e.g.
  `CODETERM_MONTEREY_LIBCXX_DIR` pointing at an existing LLVM libc++ install).
- Rebuild a Docker image from the checked-in Dockerfile.
- `gh run rerun --failed` / `gh workflow run … -f tag=vX.Y.Z-rcN` (§5).

Escalate to Andrey before doing:
- Anything touching signing/notary credentials or the minisign key.
- Provisioning a **new** runner/VM, or changing which physical machine is registered as
  `codeterm-builder` / `windows-build` / `linux-build`.
- A `master` force-push touching commits already present in both local copies, or any public
  mirror destructive op (delete a release, change the updater endpoint URL, alter the pubkey).

These mirror the catastrophic-ack list in the `github-release` skill's multi-machine protocol.

## 8. Draft/publication validation

```bash
gh release view "v$VER" --repo rollacode/codeterm --json assets,isDraft,publishedAt
gh release view "v$VER" --repo rollacode/codeterm-releases --json assets,isDraft,publishedAt
```

Before un-drafting/marking `--latest`:
- Every platform that is actually going to ship for this version has its asset(s) on **both**
  repos (private source + public mirror) — check against the artifact matrix in §3, not assumption.
- `latest.json`'s `version` field equals `$VER` and every entry in `platforms` resolves with a
  live URL (§4).
- If a platform is deliberately being shipped without (per §6, e.g. a missing Windows ARM64), that
  is a conscious exception — call it out in the release notes rather than silently publishing a
  gap.

```bash
gh release edit "v$VER" --repo rollacode/codeterm --draft=false --latest
```

## Public issue closure contract

A private-source PR or commit declares a public issue only with an exact,
repository-qualified metadata line:

```text
Public-Issue: rollacode/codeterm-releases#123
```

Use one line per issue. In PR bodies, duplicates, malformed or wrong-repository metadata, prose
`#123` references, and GitHub closing keywords fail validation. Commit messages ignore incidental
issue prose but reject malformed `Public-Issue:` metadata. The validator uses
`GH_RELEASE_TOKEN` to prove every declared issue exists in `rollacode/codeterm-releases` and
prints the normalized set in the check summary. This metadata is deliberately not a GitHub
closing keyword, so merging private source cannot close a public issue prematurely.

After publishing, `release.yml` independently verifies that the private and matching public
releases are published, that the mirror has `latest.json`, and that every version-pinned public
updater asset URL resolves. Only then does it resolve commits and associated private PRs in the
release range, union their validated metadata, and close still-open public issues. Each close
comment names `vX.Y.Z` and a declaring commit or private PR URL. Already-closed issues are skipped,
making reruns idempotent.

The close step is downstream of build, private/public publication, and mirror verification.
Queued, cancelled, skipped, failed, or mirror-unverified runs therefore execute no issue mutation.
Cross-repository reads and writes always name `rollacode/codeterm-releases` explicitly and use
`GH_RELEASE_TOKEN`; `$GITHUB_REPOSITORY` is used only to read private source PR associations.

## 9. Cleanup

```bash
git worktree list
find ../codeterm-worktrees -mindepth 1 -maxdepth 1 -type d -print
cargo sweep --time 2
```

Remove merged lane worktrees and delete their merged `lane/*` branches, close finished worker
Tabs, stop lane-specific dev daemons. Then fast-forward `develop`:

```bash
git checkout develop
git pull --ff-only origin master
git push origin develop
```

If `--ff-only` refuses, `git merge --no-ff origin/master --no-edit && git push origin develop`.
Don't skip this — a stale `develop` makes the next release start from a missing-commits base.

## Gotchas that have cost real build time (general, not version-specific)

- **Windows updater artifact is the NSIS `.exe` in Tauri v2 mode.** This repo uses
  `bundle.createUpdaterArtifacts = true`; `.nsis.zip` belongs to the old `"v1Compatible"` mode.
- **macOS notarize** needs a *real* `Developer ID Application` signature with the hardened-runtime
  flag (`flags=0x10000`) — `build-mac.sh` sanity-checks this before submitting.
- **macOS updater tar** must use `tar --no-mac-metadata`, else AppleDouble `._CodeTerm.app` shadows
  make the updater reject the bundle.
- **macOS Intel CI must use `macos-15-intel`, not `macos-13`** — GitHub retired `macos-13`.
- **`PACKAGE_REPOS_TOKEN` must have `contents:write` on `rollacode/codeterm-releases`** — the
  source release can upload with `GITHUB_TOKEN`, but the public mirror is a different repo.
- **macOS updater URLs must be arch-specific** (`CodeTerm_aarch64.app.tar.gz` /
  `CodeTerm_x64.app.tar.gz`), never both pointing at a generic `CodeTerm.app.tar.gz`.
- **`latest.json` URLs must point at `rollacode/codeterm-releases`, not the private source repo** —
  the updater runs anonymously.
- **If source release assets are valid but the mirror failed**, download from `rollacode/codeterm`,
  regenerate `latest.json` with public mirror URLs, upload with `--clobber`, then publish.

## Secrets (per builder machine)

The minisign updater key (`~/.tauri/codeterm.key`, empty password; its public half is baked into
`tauri.conf.json::plugins.updater.pubkey`) and the Apple notary credentials are backed up in mem —
the `github-release` skill lists the exact ids and restore paths. A builder without the minisign
key produces artifacts the updater can't verify.

<!-- revision: 22 -->
