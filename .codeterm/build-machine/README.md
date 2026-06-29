# CodeTerm local build machine

Build all platform artifacts **locally** (no GitHub Actions minutes), in **clean** builds,
in **batches of 3 concurrent** via a 3-engine × 2-wave model.

## Model: 3 engines × 2 waves

| Engine | Where | arm64 wave | x86_64 wave |
|--------|-------|-----------|-------------|
| **mac host** | native macOS (this Mac) | `CodeTerm_<v>_aarch64.dmg` (Developer ID + notarized) | `CodeTerm_<v>_x64.dmg` (Developer ID + notarized) |
| **docker** | baked Linux images (this Mac) | `CodeTerm_<v>_arm64.deb` | `CodeTerm_<v>_amd64.deb` (cross) |
| **windows host** | ssh into the **ROG** (remote Windows builder, Ryzen 9 7945HX) | `CodeTerm_<v>_arm64-setup.exe` | `CodeTerm_<v>_x64-setup.exe` |

**Build farm:** this Mac builds macOS (native) + Linux (Docker) only. Windows builds run on the
**ROG** (an always-on Windows machine) over SSH — there is no local Windows VM. The windows engine
pushes an auth-free git bundle to the ROG, runs `pnpm tauri build`, pulls the `.exe` back, and signs
it on the Mac.

Each engine builds **one** arch per wave → 3 builds run at once. Two waves = all 6 targets.
Builds are clean: each engine does a fresh `git clone` of the tag (via an auth-free git
bundle); no cargo target cache leaks between builds. The slow part (toolchains, system deps)
is **pre-baked** into the Docker images and the VM, so per-build startup is fast.

## Files

```
docker/Dockerfile.linux-arm64   baked aarch64 Linux env (webkit/gtk + rust 1.95 + node/pnpm)
docker/Dockerfile.linux-x64     baked x86_64 cross env (multiarch amd64 dev libs, native arm64 rustc)
scripts/build-linux.sh          thin in-container step: clone tag → pnpm install → tauri build → collect
scripts/build-windows.sh        ssh-driven remote build per arch (pushes bundle, builds NSIS, pulls .exe).
                                env: WIN_SSH/WIN_PORT/WIN_SSH_KEY + WIN_REPO_DIR/WIN_CARGO_BIN/WIN_LLVM_BIN
                                (host-agnostic; inits the repo if absent; loads the MSVC env via vcvars)
scripts/build-all.sh            orchestrator: 2 waves × 3 engines, collects + signs
```
macOS build/notarize/pack is reused from `../skills/github-release/scripts/` (`mac-bundle.sh`).

## One-time setup

```bash
cd .codeterm/build-machine/docker
docker build -t codeterm-build-linux-arm64 -f Dockerfile.linux-arm64 .
docker build -t codeterm-build-linux-x64   -f Dockerfile.linux-x64   .
```
Plus: macOS Developer ID + notary profile, `~/.tauri/codeterm.key` (minisign updater key).

**Windows host (the ROG), one-time:** OpenSSH Server installed + running + firewall open (port 22);
the Mac's build pubkey (`~/.ssh/codeterm-win-build.pub`) in `C:\ProgramData\ssh\administrators_authorized_keys`
(admin user → that file, not `%USERPROFILE%\.ssh`, with SYSTEM+Administrators-only ACL); DefaultShell =
PowerShell 7; toolchain: rustup + targets `x86_64-pc-windows-msvc` & `aarch64-pc-windows-msvc` (on the
repo-pinned toolchain), VS2022 C++ build tools (x64 **and** ARM64), node/pnpm, git, LLVM. `release.yml`
sets `WIN_SSH=amigo@<rog-ip>`, `WIN_PORT=22`, `WIN_CARGO_BIN`, `WIN_LLVM_BIN`. Key provisioning helper:
`scripts/setup-win-ssh-key.sh`.

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

The release is **tag-driven** and runs on the self-hosted runner (this Mac, labelled
`[self-hosted, codeterm-builder]` in `.github/workflows/release.yml`). It builds macOS + Linux
here and Windows on the ROG, then publishes.

1. **Land the release on `master` via a PR.** Open a PR of the release branch → `master`
   (`gh pr create --base master --head <branch>`) and merge it. Direct pushes to `master` are
   intentionally gated — always go through the PR.
2. **Bump + sync the version first** (on the branch, before the PR): set `package.json` version,
   `npm run sync:versions`, `npm run check:versions`, commit as `release: vX.Y.Z`.
3. **Tag on `master`:** `git tag release-X.Y.Z && git push origin release-X.Y.Z` (single tag — the
   workflow stamps `vX.Y.Z` itself). This triggers `release.yml`: it runs `build-all.sh` (macOS
   native + Linux Docker on the Mac, Windows over SSH on the ROG) then `publish-release.sh`.
4. **Published → done.** The workflow closes `ready-for-release` issues.

## Self-hosted runner

This Mac is registered as a self-hosted GitHub Actions runner (`codeterm-builder`). The runner
executes repo workflows locally — keep the repo private / gate fork PRs. Windows is **not** built
on this Mac (no local VM); it is offloaded to the ROG over SSH (see Build farm above).
