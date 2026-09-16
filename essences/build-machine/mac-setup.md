# Build machine setup

How this Mac was turned into a local, all-platform CodeTerm build machine — able to produce
signed/notarized macOS, Windows (NSIS), and Linux (`.deb`) artifacts **without GitHub Actions
minutes**, in clean builds, 3 architectures in parallel per wave.

> Scaffolding lives in [`.codeterm/build-machine/`](../../.codeterm/build-machine) (Dockerfiles +
> orchestrator). The release pipeline itself is the `github-release` skill
> (`.agents/skills/github-release/`). This doc is the from-scratch *setup* record.

## Topology: 3 engines × 2 waves

One physical Mac drives three independent build engines; each builds **one** arch per wave, so
three artifacts build concurrently. Two waves (arm64 family, then x86_64 family) cover all six.

| Engine | Where | Builds |
|--------|-------|--------|
| **mac host** | native Apple Silicon | macOS aarch64 + x86_64 (`.dmg` + updater `.app.tar.gz`, Developer ID + notarized) |
| **docker** | baked Linux images on the arm64 Docker host | Linux arm64 `.deb` + x86_64 `.deb` (cross) |
| **windows (ROG)** | self-hosted runner `release-windows.yml` (RETIRED Mac/QEMU/SSH path) | Windows arm64 + x86_64 NSIS `.exe` |

Builds are **clean**: each engine fresh-clones the tag (via an auth-free `git bundle`), no cargo
target cache leaks. The slow parts (toolchains, system deps) are pre-baked into the Docker images
and the VM disk, so per-build startup is fast.

Orchestrate: `.codeterm/build-machine/scripts/build-all.sh vX.Y.Z`.

---

## Engine 1 — macOS native

The Mac builds the macOS artifacts directly and is the only place the signing keys live.

**Prereqs (once):**
- Xcode + command line tools.
- `rustup target add x86_64-apple-darwin` (for the Intel cross-build).
- **Developer ID Application** cert in the login keychain:
  `security find-identity -v -p codesigning | grep ANDREY` → `Developer ID Application: ANDREY KOVALEV (JMMWQ9LWF7)`.
  (Obtained by signing into Xcode with the Apple Developer account.)
- Notary keychain profile `codeterm`:
  ```bash
  set -a; source ~/.claude/.secrets/apple-notary.env; set +a
  xcrun notarytool store-credentials codeterm \
    --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_SPECIFIC_PASSWORD"
  ```
- Minisign updater key at `~/.tauri/codeterm.key` (+ `.pub`). **The private key never leaves the
  Mac** — Windows/Linux installers are pulled back here and signed locally.

**Secrets — restore from the Bitwarden vault** (`codeterm mem secret get --name <slug>`):

| Vault slug | Restore to | chmod |
|---|---|---|
| `codeterm-apple-notary-env` | `~/.claude/.secrets/apple-notary.env` | 600 |
| `codeterm-tauri-minisign-private-key` | `~/.tauri/codeterm.key` | 600 |
| `codeterm-tauri-minisign-public-key` | `~/.tauri/codeterm.key.pub` | 644 |

(The Developer ID cert is **not** in the vault — it lives only in the login keychain. Back it up
via Keychain Access → export `.p12`.)

**Build:** `.codeterm/build-machine/scripts/build-all.sh vX.Y.Z` runs the Mac arm64 build, starts
its unchanged notarize/staple/spctl verification in the background, and compiles the x86_64 Mac
build while Apple processes the first DMG. Both architectures use `src-tauri/target`,
`CARGO_INCREMENTAL=0`, and the `embeddings` feature set; `release-inputs` prepares both the app
and embed bins in one Cargo invocation before Tauri bundles the already-built graph. Each
architecture still verifies Developer-ID + hardened-runtime + arch, notarizes + staples the DMG,
packs the `.app.tar.gz` updater bundle, and minisign-signs it.
x86_64 goes through `scripts/release-macos-x86_64.sh` (ONNX Runtime 1.23.2 Intel dylib + `CARGO_BUILD_TARGET`
plumbing — see the `macos-x86_64-release` skill).
The x86_64 embedder also bundles LLVM `libc++.1.dylib` from
`CODETERM_MONTEREY_LIBCXX_DIR` so ONNX Runtime does not depend on the newer
system libc++ float `std::to_chars` symbols missing on macOS 12 Monterey.

---

## Engine 2 — Windows (RETIRED: now the ROG self-hosted runner)

The Mac no longer builds Windows. The old path — a headless Windows-11-on-ARM QEMU guest driven
over SSH (`vm.sh`, `~/.ssh/codeterm-win-build`, `WIN_SSH*`/`WIN_VM_SSHPASS`, the
`build-windows.sh`/`setup-win-ssh-key.sh` scripts) — is **gone**. Windows now builds + signs
**natively on the ROG self-hosted runner** (`ROG-windows-build`, label `windows-build`) via
`.github/workflows/release-windows.yml`, fired by the same `release-X.Y.Z` tag. Runner toolchain
and provisioning: `.codeterm/build-machine/README.md`.

---

## Engine 3 — Linux via Docker (arm64 native + x86_64 cross)

Two baked images in [`.codeterm/build-machine/docker/`](../../.codeterm/build-machine/docker):

- **`Dockerfile.linux-arm64`** — `rust:1-trixie` + webkit2gtk-4.1 / gtk3 / etc. dev libs + Node 22
  + pnpm + Rust 1.95. Native aarch64 build → `.deb` (+ AppImage).
- **`Dockerfile.linux-x64`** — same base + `crossbuild-essential-amd64` + the **amd64 dev libs
  installed into the system via multiarch** (`dpkg --add-architecture amd64`, then
  `apt-get install --force-overwrite libwebkit2gtk-4.1-dev:amd64 …`). `rustc` runs **native arm64**;
  target code is linked for x86_64 with `x86_64-linux-gnu-gcc` against the real
  `/usr/lib/x86_64-linux-gnu`. → x86_64 `.deb`.

```bash
cd .codeterm/build-machine/docker
docker build -t codeterm-build-linux-arm64 -f Dockerfile.linux-arm64 .
docker build -t codeterm-build-linux-x64   -f Dockerfile.linux-x64   .
```

Run: `build-linux.sh <tag> [bundles]` inside the image (clone tag → `pnpm install` → `tauri build`
→ collect). **Docker Desktop memory must be ≳20 GiB** — the `codeterm` lib crate needs ~8 GiB to
compile and the OOM killer otherwise SIGKILLs `rustc`.

### Why these exact choices (hard-won)

- **x86_64 Linux is cross-compiled, never emulated.** QEMU user-mode amd64 (`docker --platform
  linux/amd64` / `cross`) **segfaults `rustc`** on an arm64 host. Native arm64 `rustc` +
  cross-linker is the only reliable path.
- **System multiarch libs, not a hand-built sysroot.** Extracting `.deb`s into a `/sysroot` misses
  the usrmerge `/lib`→`/usr/lib`, `/lib64/ld-linux-x86-64.so.2`, and transitive `.pc` chain →
  endless "cannot find … inside /sysroot" / "gdk-3.0 not found" whack-a-mole. Installing the amd64
  `-dev` libs into the real system (with `--force-overwrite` past multiarch arch-indep file
  conflicts) gives genuine system paths and resolves cleanly.
- **Cross builds need embed/cli stubs pre-created** (`touch bundled-embed/codeterm-embed-<triple>`,
  `bundled-cli/codeterm`) before `tauri build`, or its build-script glob validation aborts.
- **x86_64 AppImage cannot be built on an arm64 host** — `linuxdeploy-x86_64` is an x86_64 binary
  that can't run here. x86_64 → `.deb` only; AppImage only natively (arm64) or on a real x86_64 box.
  Hence `latest.json`'s `linux-x86_64` (an AppImage URL) needs a real x86_64 Linux runner.
- **Deb and AppImage need separate updater manifests** — Tauri's Linux platform key is only
  `linux-x86_64`, but a deb-installed app rejects AppImage bytes as `invalid updater binary
  format`. Keep AppImage in `latest.json`; publish deb packages through `latest-deb.json`.
- **AppImage in Docker** needs `APPIMAGE_EXTRACT_AND_RUN=1` (no FUSE in containers).

---

## Release pipeline (tying it together)

Driven by the `github-release` skill. Summary:

1. Bump version on master, tag `vX.Y.Z`, build from the tag.
2. Build all engines (orchestrator, or per-engine).
3. macOS: signed + notarized DMGs + `.app.tar.gz` updater bundles (minisign).
4. Windows/Linux installers pulled to the Mac, updater-signed there.
5. Upload source artifacts to private **`rollacode/codeterm`**; mirror artifacts + signed
   updater manifests to public **`rollacode/codeterm-releases`** (the GUI updater endpoint baked
   into `tauri.conf.json` points at the public mirror — private-repo assets aren't anonymously
   reachable).
6. `latest.json` covers `darwin-aarch64`, `darwin-x86_64`, `windows-x86_64` (+ `linux-x86_64`
   only when a real x86_64 Linux AppImage exists). `latest-deb.json` covers deb-installed Linux.
   Verify anon `HTTP 200` on every updater URL and that `codeterm update check` sees the new
   version.

## Tag-driven CI (implemented) + persistence

This is live as of v1.4.5. `.github/workflows/release.yml` runs on the self-hosted runner; pushing
a `release-X.Y.Z` tag builds+signs+notarizes+publishes all platforms hands-off (see the
`github-release` skill "Default release path"). Self-hosted runner minutes are free.

For the machine to survive reboots / crashes without anyone babysitting a terminal, the three
engines must each be self-starting:

1. **Runner — launchd service.** Register, then install as a service (auto-start on login +
   auto-restart on crash):
   ```bash
   cd ~/actions-runner
   TOKEN=$(gh api -X POST repos/rollacode/codeterm/actions/runners/registration-token --jq .token)
   ./config.sh --url https://github.com/rollacode/codeterm --token "$TOKEN" \
     --name codeterm-builder --labels codeterm-builder --unattended --replace
   ./svc.sh install && ./svc.sh start        # launchd LaunchAgent, RunAtLoad=true
   ```
   (Runner registration creates a persistent code-execution surface with access to keychain/vault/VM
   — a human action; keep the repo private and never run fork PRs on it.)

2. **Docker — workflow self-heals it.** The release workflow has an "Ensure Docker is running" step
   (`open -a Docker` + wait) so a down/just-rebooted Docker is started before the Linux engines.
   Optionally also enable Docker Desktop → Settings → General → "Start Docker Desktop when you sign
   in". Docker memory ≥ 11 GiB with `CARGO_BUILD_JOBS=2` (set in the images), or ≥ 20 GiB for `=6`.

3. **Windows + x86_64 Linux — their own ROG runners.** Windows builds on the ROG self-hosted runner
   (`release-windows.yml`) and x86_64 Linux on the ROG's WSL2 runner (`release-linux.yml`), both
   fired by the same `release-X.Y.Z` tag. No QEMU VM, no SSH, no `WIN_VM_SSHPASS` — each runner
   builds, signs, and publishes itself. Provisioning: `.codeterm/build-machine/README.md`.

## Redeploying on a fresh Mac (checklist)

1. Clone the repo; `brew install pnpm node gh docker` (+ Xcode).
2. Restore secrets from the vault (table above) + import the Developer ID `.p12` into the keychain;
   `xcrun notarytool store-credentials codeterm ...`.
3. `docker build` the arm64 Linux image — Engine 3 above.
4. Register + service-install the Mac runner — Engine 1 above.
5. Windows + x86_64 Linux build on the ROG runners (`release-windows.yml` / `release-linux.yml`);
   provision those separately per `.codeterm/build-machine/README.md` — nothing to do on the Mac.
6. Push a `release-0.0.0`-style test tag to validate plumbing, then a real `release-X.Y.Z`.

