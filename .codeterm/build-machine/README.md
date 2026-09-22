# CodeTerm local build machine

One `release-X.Y.Z` tag starts a single `.github/workflows/release.yml` run. Its three self-hosted
platform jobs run in parallel; the final manifest job publishes and verifies only after they all
succeed. Nothing is cross-built where a native runner exists.

## Build farm (3 self-hosted runners)

| Runner | Machine | Workflow | Builds |
|--------|---------|----------|--------|
| **`codeterm-builder`** | this Mac (ARM64) | `release.yml` → `release-builder.yml` | macOS `x64`+`arm64` (Developer ID + notarized) **and** `arm64` Linux `.deb`+`.AppImage` (Docker) |
| **`ROG-windows-build`** (`windows-build`) | ROG, Windows (x86_64) | `release.yml` → `release-windows.yml` | Windows `x64`+`arm64` NSIS, signed on the runner |
| **`linux-build`** | ROG, WSL2 Ubuntu 24.04 (x86_64) | `release.yml` → `release-linux.yml` | `x86_64` Linux `.deb`+`.AppImage`, native |

Why this split:
- **macOS** must be native (Developer ID + notarization need a Mac keychain).
- **arm64 Linux** is native-in-Docker on the ARM Mac.
- **x86_64 Linux** is built natively on the ROG's WSL2 Ubuntu — the ARM Mac can cross-build the
  `x86_64` `.deb` but **not** the `x86_64` `.AppImage` (`linuxdeploy-x86_64` is an x86_64 binary),
  so x86_64 Linux moved to the ROG to get a real AppImage.
- **Windows** is native on the ROG (no SSH, no Mac round-trip; the old SSH path is gone).

The Mac side (`build-all.sh`) runs 2 engines (mac + Docker-arm64) × 2 waves. Builds are clean: a
fresh `git clone` of the tag via an auth-free bundle; the slow toolchains/deps are pre-baked into
the Docker images so per-build startup is fast. The ROG jobs sign on their own runners via
`TAURI_SIGNING_PRIVATE_KEY`; each uploads its assets before the final manifest job updates the
mirror.

## Internal QA gates

Serial named gates for warm-checkout verification (orchestrator stride paste-back):
`scripts/qa-gate.sh` + catalog in [`QA-GATES.md`](QA-GATES.md). No daemon/socket — repo scripts only.

## Files

```
docker/Dockerfile.linux-arm64   baked aarch64 Linux env (webkit/gtk + rust 1.95 + node/pnpm)
docker/Dockerfile.linux-x64     ORPHANED — x86_64 Linux now builds natively on the ROG/WSL runner
                                (release-linux.yml), not cross-compiled here. Kept for ad-hoc use.
scripts/build-linux.sh          thin in-container step: clone tag → pnpm install → tauri build → collect
scripts/build-all.sh            orchestrator: Mac overlap + optional arm64-Linux Docker, signs
scripts/mac-overlap.sh           Mac arm64 notarization overlaps the x86_64 build
```
Windows + x86_64-Linux release builds run on the ROG self-hosted runners
(`release-windows.yml` / `release-linux.yml`); there is no Mac-driven SSH/QEMU path anymore.
macOS build/notarize/pack is reused from `../skills/github-release/scripts/` (`mac-bundle.sh`);
the overlap coordinator is `scripts/mac-overlap.sh`.

## One-time setup

```bash
cd .codeterm/build-machine/docker
docker build -t codeterm-build-linux-arm64 -f Dockerfile.linux-arm64 .
docker build -t codeterm-build-linux-x64   -f Dockerfile.linux-x64   .
```
Plus: macOS Developer ID + notary profile, `~/.tauri/codeterm.key` (minisign updater key).

**Windows host (the ROG), one-time:** registered as a **self-hosted GitHub Actions runner**
(`ROG-windows-build`, labels include `windows-build`) — NOT driven over SSH. Toolchain on the runner:
rustup on the repo-pinned channel (1.95.0) with targets `x86_64-pc-windows-msvc` &
`aarch64-pc-windows-msvc` installed, VS2022 C++ build tools (x64 **and** ARM64 — the aarch64 build
loads `vcvarsall x64_arm64`), node 22 + pnpm 10.32.1, git, LLVM. Windows signing uses the
`TAURI_SIGNING_PRIVATE_KEY` / optional `TAURI_KEY_PASSWORD` repo secrets directly on the runner;
publishing to the mirror uses `PACKAGE_REPOS_TOKEN`. No SSH, no remote-build env — the old
Mac→Windows SSH/QEMU path is gone.

**x86_64 Linux runner (the ROG's WSL2), one-time:** install WSL2 with **Ubuntu 24.04** (glibc 2.39 —
required by the onnxruntime static lib), register a **self-hosted GitHub Actions runner** with label
`linux-build`, and install rustup on the repo-pinned channel (1.95.0), node 22 + pnpm 10.32.1, `gh`,
and the apt deps listed in `release-linux.yml` (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`,
`libayatana-appindicator3-dev`, `librsvg2-dev`, `libssl-dev`, `patchelf`). Same signing/mirror
secrets as Windows. Runs natively → produces the `x86_64` AppImage the ARM Mac can't.

## Run

```bash
.codeterm/build-machine/scripts/build-all.sh v1.4.4
# artifacts land in /tmp/codeterm-<ver>-artifacts/
```

## Hard-won notes / limitations

- **Linux x86_64 AppImage cannot be built on an arm64 host** (`linuxdeploy-x86_64` is an x86_64
  binary that can't run here) — which is exactly why x86_64 Linux moved to the ROG/WSL native
  runner (`release-linux.yml`). The historical Mac approach (cross `.deb` only, via
  `Dockerfile.linux-x64` with multiarch amd64 libs — no sysroot) is retired from the release path;
  the Docker x64 image is kept only for ad-hoc local use.
- **Cross builds need embed/cli stubs pre-created** or `tauri-build` glob validation fails
  (`resource path bundled-embed/codeterm-embed-<triple> doesn't exist`). Handled in the scripts and
  in `release-windows.yml` (the aarch64 cross build pre-creates the target-triple sidecar stubs).
- **Release compilation uses one target contract.** Each platform job sets `CARGO_TARGET_DIR` to the
  checkout's `src-tauri/target`; cross-target lanes carry their target in `CARGO_BUILD_TARGET`,
  and all jobs pin the `embeddings` features with `CARGO_INCREMENTAL=0`. `release-inputs` builds both application bins
  before Tauri bundles them, so the dependency graph is not compiled a second time. The three
  platform workflows intentionally have no Rust target-cache action or target upload.
- **Mac notarization overlaps architectures.** `mac-overlap.sh` completes and verifies the arm64
  app, starts the existing `notarize-dmg.sh` (including stapler and spctl), then runs the complete
  x86_64 pipeline. The arm64 updater bundle is packed only after its notarization succeeds.
- **The final manifest job runs on `linux-build`.** It has bash, jq, gh and python3; it runs only
  after the Mac, Linux x86_64 and Windows platform jobs succeed, then verifies the public manifests.
- **Timing ledger.** The 1.10.22 baseline was Mac 418s arm64 + 427s x86_64 sequential, Windows
  release-input/Tauri 7m04s/5m30s (arm64) and 8m01s/5m15s (x86_64), and Linux 18m45s/16m05s;
  target-cache saves added 42s, 43s, and 1m33s. Record the next real release's corresponding
  before/after step durations and tag-to-publish time with `gh api .../actions/runs/<id>/jobs`.
  No after-release timing is claimed until that release runs.
- **Updater signing is per-runner via a secret.** `TAURI_SIGNING_PRIVATE_KEY` signs the `.sig`
  files: the ROG Windows/Linux runners sign directly; the Mac currently signs AppImage with the
  local `~/.tauri/codeterm.key` (could use the same secret). Only **Apple notarization** is
  genuinely Mac-bound (Developer ID + keychain) — minisign is not.
- **Docker memory:** the `codeterm` lib crate needs ≳8 GiB to compile; set Docker Desktop
  memory to ~20 GiB or it OOM-kills `rustc` (SIGKILL).

## Release runbook (PR-based, tag-driven)

The release is **tag-driven**. One `release-X.Y.Z` push creates a single workflow run with three
parallel platform jobs on their self-hosted runners:

- `.github/workflows/release.yml` calls `release-builder.yml` on the **Mac** (`[self-hosted, codeterm-builder]`) — builds
  macOS + arm64 Linux via `build-all.sh`.
- `.github/workflows/release.yml` calls `release-windows.yml` on the **ROG** (`[self-hosted, windows-build]`) — builds
  + signs the Windows installers for **both** arches.
- `.github/workflows/release.yml` calls `release-linux.yml` on the **ROG's WSL2** (`[self-hosted, linux-build]`) —
  builds + signs `x86_64` Linux `.deb`+`.AppImage` natively.

The `release-manifest` job needs all three platform jobs. It merges every updater fragment, updates
the source and public manifests, marks the releases, verifies public served bytes, and then applies
the public issue-closure contract. A platform failure skips that job and leaves the release draft.

0. **Preflight the release head.** The release branch is cut from `master` and dies at the tag, so
   it cannot drift from it — verify it is still descended from `origin/master`
   (`git merge-base --is-ancestor origin/master origin/release/X.Y.Z`) and, if a hotfix landed on
   `master` meanwhile, merge `master` in, run gates and push before continuing. Visually check dev
   from the main checkout, and clean merged lane worktrees under `../codeterm-worktrees/` so old
   artifacts and stale agents do not influence the release.
1. **Land the release on `master` via a PR.** Open a PR of the release branch → `master`
   (`gh pr create --base master --head <branch>`) and merge it. Direct pushes to `master` are
   intentionally gated — always go through the PR.
2. **Bump + sync the version first** (on the branch, before the PR): set `package.json` version,
   `npm run sync:versions`, `npm run check:versions`, commit as `release: vX.Y.Z`.
3. **Tag on the merged `master` SHA:** after the PR is green and merged, verify local `HEAD` and
   `origin/master` equal the intended merge SHA, verify remote `release-X.Y.Z` and `vX.Y.Z` are
   absent, then `git tag release-X.Y.Z <merged-master-sha> && git push origin release-X.Y.Z`.
   That one tag starts all three platform jobs in a single `release.yml` run.
4. **Published → done.** The run's final manifest job verifies the public updater manifests before
   closing eligible `ready-for-release` issues.

Recovery rule: do not move or overwrite a published release tag/artifact by default. If a bad
public release is intentionally deleted, cut the fix as a new patch version through the same
release branch → master PR path and trigger it with a new `release-X.Y.Z` tag.

Closeout rule: once artifacts publish, remove merged lane worktrees under `../codeterm-worktrees/`,
delete merged `lane/*` branches, close finished worker tabs, stop lane-specific dev daemons, and
run `cargo sweep --time 2` on build machines/checkouts that accumulated Rust/Tauri artifacts. Old
worktrees and `target/` layers are release debt, not something to leave for the next run.

> **Manual re-run / verification:** dispatch `release.yml` with `tag=release-X.Y.Z`. Set
> `mirror=false` to leave releases in draft and skip the public mirror; the caller still builds and
> uploads private artifacts so the platform jobs can be inspected in one run.

## Self-hosted runners

Three self-hosted GitHub Actions runners, each assigned to its platform job by label:

- **Mac** (`codeterm-builder`) — `release.yml` (macOS + arm64 Linux).
- **ROG / Windows** (`ROG-windows-build`, label `windows-build`) — `release-windows.yml` (Windows
  x64 + arm64, built + signed on the runner).
- **ROG / WSL2 Ubuntu** (label `linux-build`) — `release-linux.yml` (x86_64 Linux, native).

No SSH between machines; each runner signs and uploads its platform assets, and the final manifest
job publishes the verified release. Keep the repo private / gate fork PRs — self-hosted runners
execute repo workflow code.

<!-- revision: 2 -->
