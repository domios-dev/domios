# CodeTerm local build machine

Build the macOS + Linux artifacts **locally** (no GitHub Actions minutes), in **clean** builds,
in **batches of 2 concurrent** via a 2-engine × 2-wave model. **Windows is NOT built here** — it
runs on the ROG self-hosted GitHub Actions runner (see *Build farm* below).

## Model: 2 engines × 2 waves (mac + linux)

| Engine | Where | arm64 wave | x86_64 wave |
|--------|-------|-----------|-------------|
| **mac host** | native macOS (this Mac) | `CodeTerm_<v>_aarch64.dmg` (Developer ID + notarized) | `CodeTerm_<v>_x64.dmg` (Developer ID + notarized) |
| **docker** | baked Linux images (this Mac) | `CodeTerm_<v>_arm64.deb` | `CodeTerm_<v>_amd64.deb` (cross) |

**Build farm:** this Mac (self-hosted runner `codeterm-builder`) builds macOS (native) + Linux
(Docker) only. **Windows builds run on the ROG** (an always-on Windows machine, Ryzen 9 7945HX)
registered as its OWN self-hosted GitHub Actions runner (`ROG-windows-build`, label `windows-build`).
Windows is driven by `.github/workflows/release-windows.yml` — tag-triggered, builds **both** arches
(`x86_64` + `aarch64`), signs the NSIS installers on the runner via `TAURI_SIGNING_PRIVATE_KEY`, and
publishes them itself. No SSH from the Mac, no Mac round-trip.

Each Mac engine builds **one** arch per wave → 2 builds run at once. Two waves = all 4 mac+linux
targets. Builds are clean: each engine does a fresh `git clone` of the tag (via an auth-free git
bundle); no cargo target cache leaks between builds. The slow part (toolchains, system deps)
is **pre-baked** into the Docker images, so per-build startup is fast.

## Files

```
docker/Dockerfile.linux-arm64   baked aarch64 Linux env (webkit/gtk + rust 1.95 + node/pnpm)
docker/Dockerfile.linux-x64     baked x86_64 cross env (multiarch amd64 dev libs, native arm64 rustc)
scripts/build-linux.sh          thin in-container step: clone tag → pnpm install → tauri build → collect
scripts/build-all.sh            orchestrator: 2 waves × 2 engines (mac+linux), collects + signs
scripts/build-windows.sh        ORPHANED manual tool — NOT called by build-all.sh anymore. ssh-driven
                                remote build per arch; kept only for ad-hoc manual Windows builds.
                                Release Windows builds go through release-windows.yml on the ROG runner.
```
macOS build/notarize/pack is reused from `../skills/github-release/scripts/` (`mac-bundle.sh`).

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
publishing to the mirror uses `PACKAGE_REPOS_TOKEN`. No SSH key, no `WIN_SSH`/`WIN_*` env.

## Run

```bash
.codeterm/build-machine/scripts/build-all.sh v1.4.4
# artifacts land in /tmp/codeterm-<ver>-artifacts/
```

## Hard-won notes / limitations

- **Linux x86_64 = cross-compile, NOT emulation.** QEMU user-mode amd64 segfaults `rustc` on
  an arm64 host. The working approach: native arm64 `rustc`, link target code with
  `x86_64-linux-gnu-gcc` against **multiarch amd64 system libs** (no hand-built sysroot —
  a sysroot misses the usrmerge `/lib`,`/lib64`,`ld-linux` symlinks and turns into whack-a-mole).
  Done inside `Dockerfile.linux-x64`.
- **Linux x86_64 AppImage cannot be built on an arm64 host** (`linuxdeploy-x86_64` is an x86_64
  binary that can't run here). x86_64 → `.deb` only. AppImage only for arm64, or on a real
  x86_64 host. So `latest.json`'s `linux-x86_64` (AppImage) needs a real x86_64 Linux box.
- **Cross builds need embed/cli stubs pre-created** or `tauri-build` glob validation fails
  (`resource path bundled-embed/codeterm-embed-<triple> doesn't exist`). Handled in the scripts.
- **Updater signing happens on the Mac.** The minisign private key never leaves the Mac;
  Windows/Linux installers are pulled back and signed here.
- **Docker memory:** the `codeterm` lib crate needs ≳8 GiB to compile; set Docker Desktop
  memory to ~20 GiB or it OOM-kills `rustc` (SIGKILL).

## Release runbook (PR-based, tag-driven)

The release is **tag-driven**. The SAME `release-X.Y.Z` tag fires two workflows in parallel,
each on its own self-hosted runner:

- `.github/workflows/release.yml` on the **Mac** (`[self-hosted, codeterm-builder]`) — builds
  macOS + Linux via `build-all.sh`, then `publish-release.sh`.
- `.github/workflows/release-windows.yml` on the **ROG** (`[self-hosted, windows-build]`) — builds
  + signs the Windows installers for **both** arches, then publishes/mirrors them.

1. **Land the release on `master` via a PR.** Open a PR of the release branch → `master`
   (`gh pr create --base master --head <branch>`) and merge it. Direct pushes to `master` are
   intentionally gated — always go through the PR.
2. **Bump + sync the version first** (on the branch, before the PR): set `package.json` version,
   `npm run sync:versions`, `npm run check:versions`, commit as `release: vX.Y.Z`.
3. **Tag on `master`:** `git tag release-X.Y.Z && git push origin release-X.Y.Z` (single tag — both
   workflows stamp/map `vX.Y.Z` themselves). `release.yml` runs `build-all.sh` (macOS native + Linux
   Docker on the Mac) then `publish-release.sh`; `release-windows.yml` builds x64 + arm64 on the ROG.
4. **Published → done.** `release.yml` closes `ready-for-release` issues.

> **Manual Windows re-run / verification:** `gh workflow run release-windows.yml --ref <branch>
> -f tag=vX.Y.Z-rcN` runs ONLY the Windows workflow via `workflow_dispatch` (a `-rc` tag routes to
> `latest-rc.json` and the mirror step is skipped), so you can validate a Windows build on the ROG
> without touching the stable channel.

## Self-hosted runners

Two self-hosted GitHub Actions runners, each scoped to its workflow by label:

- **Mac** (`codeterm-builder`) — runs `release.yml` (macOS + Linux).
- **ROG** (`ROG-windows-build`, label `windows-build`) — runs `release-windows.yml` (Windows x64 +
  arm64, built + signed on the runner). No SSH from the Mac; the ROG signs and publishes itself.

Keep the repo private / gate fork PRs — self-hosted runners execute repo workflow code.
