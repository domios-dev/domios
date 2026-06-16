# Build machine setup

How this Mac was turned into a local, all-platform CodeTerm build machine — able to produce
signed/notarized macOS, Windows (NSIS), and Linux (`.deb`) artifacts **without GitHub Actions
minutes**, in clean builds, 3 architectures in parallel per wave.

> Scaffolding lives in [`.codeterm/build-machine/`](../.codeterm/build-machine/) (Dockerfiles +
> orchestrator). The release pipeline itself is the `github-release` skill
> (`.codeterm/skills/github-release/`). This doc is the from-scratch *setup* record.

## Topology: 3 engines × 2 waves

One physical Mac drives three independent build engines; each builds **one** arch per wave, so
three artifacts build concurrently. Two waves (arm64 family, then x86_64 family) cover all six.

| Engine | Where | Builds |
|--------|-------|--------|
| **mac host** | native Apple Silicon | macOS aarch64 + x86_64 (`.dmg` + updater `.app.tar.gz`, Developer ID + notarized) |
| **docker** | baked Linux images on the arm64 Docker host | Linux arm64 `.deb` + x86_64 `.deb` (cross) |
| **windows VM** | QEMU Windows-11-ARM, driven over SSH | Windows arm64 + x86_64 NSIS `.exe` |

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

**Build:** `.codeterm/skills/github-release/scripts/mac-bundle.sh <ver> {arm64|x86_64} <workdir>`
— builds, verifies Developer-ID + hardened-runtime + arch, notarizes + staples the DMG, packs the
`.app.tar.gz` updater bundle, minisign-signs it, echoes the signature for `latest.json`.
x86_64 goes through `scripts/release-macos-x86_64.sh` (ONNX Runtime 1.23.2 Intel dylib + `CARGO_BUILD_TARGET`
plumbing — see the `macos-x86_64-release` skill).

---

## Engine 2 — Windows 11 ARM64 VM (QEMU + HVF)

A headless Windows-11-on-ARM guest on Apple Silicon, reached over SSH. Lives in
`~/VMs/codeterm-win11-arm/`, launched by `vm.sh`.

### 2a. Create the VM

```
~/VMs/codeterm-win11-arm/
  vm.sh              launcher (install | run | screen | stop)
  vm-ssh.sh          sshpass wrapper -> test@127.0.0.1:2222
  vm-scp.sh          scp wrapper
  autounattend.xml   unattended Windows install answer file (creates user test/test, enables OpenSSH)
  Fido.ps1           downloads the official Windows 11 ARM64 ISO
  win11-arm64.iso    Windows 11 ARM installer
  virtio-win.iso     VirtIO drivers (storage/net) for the installer
  win11.qcow2        80 GB system disk
  efi-vars.fd        64 MB EDK2 NVRAM store
  tpm/               swtpm (emulated TPM 2.0 — Win11 hard requirement)
```

Tooling: `brew install qemu swtpm`. EDK2 aarch64 firmware ships with QEMU at
`$(brew --prefix)/share/qemu/edk2-aarch64-code.fd`. Get the ISO with `Fido.ps1` (or download the
Win11 ARM64 ISO manually); virtio-win ISO from the Fedora virtio-win repo.

The launcher (`vm.sh`) — key QEMU shape:

```bash
qemu-system-aarch64 -M virt -accel hvf -cpu host -smp 6 -m 8192 \
  -drive if=pflash,format=raw,readonly=on,file=.../share/qemu/edk2-aarch64-code.fd \
  -drive if=pflash,format=raw,file=efi-vars.fd \
  -device nvme,drive=disk0,serial=cafebabe -drive id=disk0,if=none,file=win11.qcow2,format=qcow2 \
  -chardev socket,id=chrtpm,path=tpm/swtpm.sock -tpmdev emulator,id=tpm0,chardev=chrtpm \
  -device tpm-tis-device,tpmdev=tpm0 \
  -device virtio-net-pci,netdev=net0 \
  -netdev user,id=net0,hostfwd=tcp:127.0.0.1:2222-:22 \
  -device qemu-xhci -device usb-kbd -device usb-tablet \
  -device ramfb \
  -display none -vnc 127.0.0.1:7,password-secret=vncpw \
  -qmp unix:qmp.sock,server,nowait
```

- **`swtpm` + `tpm-tis-device`** — Windows 11 refuses to install without TPM 2.0.
- **`-device ramfb`** — gives a framebuffer so the installer/desktop is visible over VNC. Without
  a display device the EDK2 NVRAM init can stall before the bootloader (black screen — a real
  trap we hit; `ramfb` or `virtio-gpu-pci` fixes it).
- **SLIRP user-net + `hostfwd tcp:127.0.0.1:2222-:22`** — SSH on `localhost:2222`, no privileges
  needed. SLIRP is slow; for faster builds switch to `-netdev vmnet-shared` (needs sudo/entitlement).
- **Install is unattended**: `autounattend.xml` is exposed to setup as a FAT volume via
  `-drive file=fat:rw:...` so Windows finds it at a drive root. It creates user **`test`** /
  password **`test`**, accepts EULA, and enables the OpenSSH server.
- Display/VNC on `127.0.0.1:5907` (`:7`), password `1234`; `screen`/`stop` go via the QMP socket.

```bash
./vm.sh install   # first boot, ~unattended Windows install
./vm.sh run       # normal headless boot, SSH on :2222
```

### 2b. Install the Windows build toolchain (inside the guest)

Over SSH (`vm-ssh.sh`) or VNC console. Several steps need **Administrator** (UAC), so run them
from an elevated console / self-elevating `.cmd` on the desktop (a non-elevated SSH session can't
install into `Program Files`):

- **Rust** (rustup) → installs `aarch64-pc-windows-msvc` by default.
- **Visual Studio Build Tools 2022** with the **MSVC v143 + Windows SDK** workload. For x86_64
  cross-builds also add the x64/x86 component:
  ```
  vs_installer.exe modify --installPath "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools" \
    --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64 --quiet --norestart --wait
  ```
- **git**, **Node 22+**, **LLVM/clang** (clang is required by the `ring` crate) via winget:
  ```
  winget install Git.Git OpenJS.NodeJS.LTS LLVM.LLVM -e --silent --accept-package-agreements --accept-source-agreements
  corepack enable && corepack prepare pnpm@latest --activate
  ```
- Rust targets for both arches on the pinned toolchain:
  `rustup target add aarch64-pc-windows-msvc x86_64-pc-windows-msvc --toolchain 1.95.0`

### 2c. Build (driven from the Mac)

`.codeterm/build-machine/scripts/build-windows.sh <tag> {x86_64|aarch64}` pushes a git bundle,
fetches+checkouts the tag, `pnpm install`, pre-creates the embed/cli **stubs** (else `tauri-build`
glob validation fails with `resource path bundled-embed/codeterm-embed-<triple> doesn't exist`),
`pnpm tauri build --target <triple>`, and pulls the `.exe` back. The NSIS `.exe` is signed for the
updater **on the Mac** afterwards (`pnpm tauri signer sign`).

---

## Engine 3 — Linux via Docker (arm64 native + x86_64 cross)

Two baked images in [`.codeterm/build-machine/docker/`](../.codeterm/build-machine/docker/):

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
- **AppImage in Docker** needs `APPIMAGE_EXTRACT_AND_RUN=1` (no FUSE in containers).

---

## Release pipeline (tying it together)

Driven by the `github-release` skill. Summary:

1. Bump version on master, tag `vX.Y.Z`, build from the tag.
2. Build all engines (orchestrator, or per-engine).
3. macOS: signed + notarized DMGs + `.app.tar.gz` updater bundles (minisign).
4. Windows/Linux installers pulled to the Mac, updater-signed there.
5. Upload source artifacts to private **`rollacode/codeterm`**; mirror artifacts + a signed
   **`latest.json`** to public **`rollacode/codeterm-releases`** (the GUI updater endpoint baked
   into `tauri.conf.json` points at the public mirror — private-repo assets aren't anonymously
   reachable).
6. `latest.json` covers `darwin-aarch64`, `darwin-x86_64`, `windows-x86_64` (+ `linux-x86_64`
   only when a real x86_64 Linux AppImage exists). Verify anon `HTTP 200` on every updater URL and
   that `codeterm update check` sees the new version.

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

3. **Windows VM — NOT auto-started yet.** `~/VMs/codeterm-win11-arm/vm.sh run` is launched manually
   (foreground qemu). To make it self-healing like the runner, wrap it in a launchd LaunchAgent
   (`RunAtLoad` + `KeepAlive`) running `vm.sh run` (headless qemu under the user GUI session, HVF
   works). Until then, after a reboot/crash run `vm.sh run` by hand or the Windows engine fails
   (preflight will warn `127.0.0.1:2222` unreachable). The repo secret `WIN_VM_SSHPASS` holds the VM
   password for the Windows engine.

## Redeploying on a fresh Mac (checklist)

1. Clone the repo; `brew install qemu swtpm pnpm node gh docker` (+ Xcode).
2. Restore secrets from the vault (table above) + import the Developer ID `.p12` into the keychain;
   `xcrun notarytool store-credentials codeterm ...`.
3. Build the Windows VM (`~/VMs/codeterm-win11-arm/vm.sh install`, then `run`) — Engine 2 above.
4. `docker build` the two Linux images — Engine 3 above.
5. Register + service-install the runner — Engine 1 above.
6. Set repo secret `WIN_VM_SSHPASS`; `net accounts /lockoutthreshold:0` in the VM (no SSH lockout).
7. Push a `release-0.0.0`-style test tag to validate plumbing, then a real `release-X.Y.Z`.

<!-- revision: 2 -->
