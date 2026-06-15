# CodeTerm local build machine

Build all platform artifacts **locally** (no GitHub Actions minutes), in **clean** builds,
in **batches of 3 concurrent** via a 3-engine × 2-wave model.

## Model: 3 engines × 2 waves

| Engine | Where | arm64 wave | x86_64 wave |
|--------|-------|-----------|-------------|
| **mac host** | native macOS | `CodeTerm_<v>_aarch64.dmg` (Developer ID + notarized) | `CodeTerm_<v>_x64.dmg` (Developer ID + notarized) |
| **docker** | baked Linux images | `CodeTerm_<v>_arm64.deb` | `CodeTerm_<v>_amd64.deb` (cross) |
| **windows VM** | ssh into QEMU/peer | `CodeTerm_<v>_arm64-setup.exe` | `CodeTerm_<v>_x64-setup.exe` |

Each engine builds **one** arch per wave → 3 builds run at once. Two waves = all 6 targets.
Builds are clean: each engine does a fresh `git clone` of the tag (via an auth-free git
bundle); no cargo target cache leaks between builds. The slow part (toolchains, system deps)
is **pre-baked** into the Docker images and the VM, so per-build startup is fast.

## Files

```
docker/Dockerfile.linux-arm64   baked aarch64 Linux env (webkit/gtk + rust 1.95 + node/pnpm)
docker/Dockerfile.linux-x64     baked x86_64 cross env (multiarch amd64 dev libs, native arm64 rustc)
scripts/build-linux.sh          thin in-container step: clone tag → pnpm install → tauri build → collect
scripts/build-windows.sh        ssh-driven VM build per arch (pushes bundle, builds NSIS, pulls .exe)
scripts/build-all.sh            orchestrator: 2 waves × 3 engines, collects + signs
```
macOS build/notarize/pack is reused from `../skills/github-release/scripts/` (`mac-bundle.sh`).

## One-time setup

```bash
cd .codeterm/build-machine/docker
docker build -t codeterm-build-linux-arm64 -f Dockerfile.linux-arm64 .
docker build -t codeterm-build-linux-x64   -f Dockerfile.linux-x64   .
```
Plus: macOS Developer ID + notary profile, `~/.tauri/codeterm.key` (minisign updater key),
and the Windows VM reachable over ssh (`WIN_SSH`/`WIN_PORT`/`SSHPASS`).

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

## Future: self-hosted GitHub Actions runner

Register this Mac as a self-hosted runner (Settings → Actions → Runners → New runner) and wrap
the above in `.github/workflows/release.yml` so `git push` / tag triggers the matrix. Then it's
a real CI build machine usable by anyone, no agent needed. (Runner registration token is
user-provided; a self-hosted runner executes repo workflows on this machine — keep the repo
private / gate fork PRs.)
