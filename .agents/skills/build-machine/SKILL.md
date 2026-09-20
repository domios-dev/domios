---
name: build-machine
description: "Provision or restore CodeTerm self-hosted build machines and runners."
---

## Build machine setup

How the Mac and ROG runners produce
signed/notarized macOS, Windows (NSIS), and Linux (`.deb`) artifacts **without GitHub Actions
minutes**, in clean builds.

> Scaffolding lives in [`.codeterm/build-machine/`](../../.codeterm/build-machine) (Dockerfiles +
> orchestrator). The release pipeline itself is the `github-release` skill
> (`.agents/skills/github-release/`). This doc is the from-scratch *setup* record.

## Topology: platform jobs across three runners

The single release workflow starts three platform jobs in parallel. The Mac job overlaps its two
macOS architectures and builds arm64 Linux in Docker; the ROG runners build x86_64 Linux and both
Windows architectures.

| Engine | Where | Builds |
|--------|-------|--------|
| **mac host + Docker** | native Apple Silicon and baked Linux arm64 image | macOS aarch64 + x86_64 (`.dmg` + updater `.app.tar.gz`, Developer ID + notarized); Linux arm64 `.deb` + `.AppImage` |
| **Linux (ROG/WSL2)** | self-hosted runner `release-linux.yml` | Linux x86_64 `.deb` + `.AppImage`, native |
| **Windows (ROG)** | self-hosted runner `release-windows.yml` (RETIRED Mac/QEMU/SSH path) | Windows arm64 + x86_64 NSIS `.exe` |

Builds are **clean**: each engine fresh-clones the tag (via an auth-free `git bundle`), no cargo
target cache leaks. The slow parts (toolchains, system deps) are pre-baked into the Docker images
and the VM disk, so per-build startup is fast.

Orchestrate: `.codeterm/build-machine/scripts/build-all.sh vX.Y.Z`.

---

## Engine 1 — macOS native

The Mac builds and signs macOS artifacts; its local minisign key also signs the arm64 Linux
AppImage.

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
- Minisign updater key at `~/.tauri/codeterm.key` (+ `.pub`) for macOS and arm64 Linux. Windows and
  x86_64 Linux updater bundles use `TAURI_SIGNING_PRIVATE_KEY` from Actions secrets on their runners.

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
the reusable Windows job in `.github/workflows/release-windows.yml`, called by the single
`.github/workflows/release.yml` run for `release-X.Y.Z`. Runner toolchain and provisioning:
`.codeterm/build-machine/README.md`.

---

## Engine 3 — Linux via Docker (arm64 native; x86_64 on ROG/WSL2)

Two baked images in [`.codeterm/build-machine/docker/`](../../.codeterm/build-machine/docker):

- **`Dockerfile.linux-arm64`** — `rust:1-trixie` + webkit2gtk-4.1 / gtk3 / etc. dev libs + Node 22
  + pnpm + Rust 1.95. Native aarch64 build → `.deb` (+ AppImage).
- **`Dockerfile.linux-x64`** — retained for ad-hoc cross-builds. Release x86_64 Linux builds
  natively on the ROG's WSL2 Ubuntu runner because the x86_64 AppImage tool cannot run on arm64.

```bash
cd .codeterm/build-machine/docker
docker build -t codeterm-build-linux-arm64 -f Dockerfile.linux-arm64 .
docker build -t codeterm-build-linux-x64   -f Dockerfile.linux-x64   .
```

Run: `build-linux.sh <tag> [bundles]` inside the image for arm64 release builds or ad-hoc
cross-builds (clone tag → `pnpm install` → `tauri build` → collect). **Docker Desktop memory must
be ≳20 GiB** — the `codeterm` lib crate needs ~8 GiB to compile and the OOM killer otherwise
SIGKILLs `rustc`.

### Why these exact choices (hard-won)

- **Release x86_64 Linux runs natively on the ROG's WSL2 runner.** The retained Docker x64 image
  uses native arm64 `rustc` plus a cross-linker for ad-hoc `.deb` builds; QEMU user-mode amd64
  (`docker --platform linux/amd64` / `cross`) **segfaults `rustc`** on an arm64 host.
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

1. Bump version on master and push the single `release-X.Y.Z` trigger tag.
2. One `release.yml` run prepares draft releases, then starts Mac + arm64 Linux, x86_64 Linux,
   and Windows platform jobs in parallel.
3. macOS builds signed + notarized DMGs and `.app.tar.gz` updater bundles; Linux builds AppImage
   and Debian packages; Windows builds signed NSIS installers.
4. Each platform job uploads its source artifacts and mirrors large assets. The final manifest job
   waits for every platform job, merges the updater fragments, publishes signed manifests to the
   public **`rollacode/codeterm-releases`** mirror, marks releases, and verifies served bytes.
5. `latest.json` keeps the Tauri updater platform records; `latest-deb.json` remains separate for
   Debian-installed Linux. Verification checks every shipped updater URL before issue closure.

## Tag-driven CI (implemented) + persistence

This is live as of v1.4.5. Pushing a `release-X.Y.Z` tag starts one `.github/workflows/release.yml`
run with parallel self-hosted platform jobs and a final manifest verifier (see the `github-release`
skill "Default release path"). Self-hosted runner minutes are free.

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

3. **Windows + x86_64 Linux — their own ROG runners.** The single release run calls the reusable
   Windows job (`release-windows.yml`) and x86_64 Linux job (`release-linux.yml`) on the ROG's
   self-hosted runners. No QEMU VM, no SSH, no `WIN_VM_SSHPASS`; each platform job builds and signs
   its artifacts before the final manifest job publishes the release. Provisioning:
   `.codeterm/build-machine/README.md`.

## Redeploying on a fresh Mac (checklist)

1. Clone the repo; `brew install pnpm node gh docker` (+ Xcode).
2. Restore secrets from the vault (table above) + import the Developer ID `.p12` into the keychain;
   `xcrun notarytool store-credentials codeterm ...`.
3. `docker build` the arm64 Linux image — Engine 3 above.
4. Register + service-install the Mac runner — Engine 1 above.
5. Windows + x86_64 Linux build on the ROG runners (`release-windows.yml` / `release-linux.yml`);
   provision those separately per `.codeterm/build-machine/README.md` — nothing to do on the Mac.
6. Push a `release-0.0.0`-style test tag to validate plumbing, then a real `release-X.Y.Z`.

---

## Windows Runner Bootstrap

This runbook restores the ROG Windows GitHub Actions runner after a clean Windows install or OS wipe. It keeps secrets external, installs non-secret tools into user-writable paths, writes only the runner-local `actions-runner/.env`, and can register the runner with an ephemeral token supplied at runtime.

## Operator Quickstart

1. Open PowerShell as the runner user.
2. Install GitHub CLI or Git if missing enough to fetch this repository.
3. Get a one-use GitHub Actions runner registration token outside the script. Do not save it in files or paste it into logs.
4. Run the bootstrap from the repository checkout:

```powershell
.\scripts\windows-runner-bootstrap.ps1 `
  -InstallVisualStudioBuildTools `
  -ConfigureRunner `
  -Replace `
  -RegistrationTokenCommand 'codeterm paste get paste:REPLACE_ME --pop' `
  -StartListener
```

For service mode, use an elevated shell and add `-RunAsService`. Without elevation, keep the interactive listener running. Service installation is the only supported persistent background mode; do not invent a separate scheduler around `run.cmd`.

## What The Script Installs

User-writable tools:

- Official GitHub Actions runner `win-x64`, under `%USERPROFILE%\actions-runner`.
- Official PowerShell `win-x64` zip, under `%USERPROFILE%\tools\PowerShell\<version>`.
- Official Node.js active LTS `win-x64` zip, under `%USERPROFILE%\tools\Node\<version>`.
- Rust via official `https://win.rustup.rs/x86_64`, with stable MSVC and the repository-pinned `1.95.0-x86_64-pc-windows-msvc` toolchain.
- `pnpm` at the repository-pinned version, plus command shims under `%USERPROFILE%\tools\runner-shims`.
- Visual Studio Build Tools C++ workload and Windows SDK when `-InstallVisualStudioBuildTools` is provided from an elevated shell.

Runner-local environment:

- `%USERPROFILE%\actions-runner\.env` prepends shims, Node, PowerShell, durable Rust proxies, and Cargo before machine/user PATH.
- The `.env` file is runner-local and is the persistent mechanism that makes the Actions shell see `pwsh`, `rustup`, `cargo`, `rustc`, `node`, `npm`, `corepack`, `pnpm`, and `git`.
- Durable Rust proxies live under `%USERPROFILE%\tools\rustup-proxies`, outside Cargo's cache path.

## Incident Record

| Symptom | Root Cause | Automated check/fix | Manual escalation boundary |
| --- | --- | --- | --- |
| No runner existed after OS wipe; GitHub had no online Windows builder. | Runner install and registration state was lost with the OS. | Download official `actions/runner` `win-x64`, configure with repo URL, name, labels, and caller-supplied one-use token. | If service mode is required, rerun elevated with `-RunAsService`; otherwise keep `run.cmd` listener alive. |
| Runner registration and `.env` are intact, but GitHub reports it offline after a CodeTerm restart. | A per-user Startup launcher owned `run.cmd`; no official runner service or `Runner.Listener` survived the restart. | Preserve `.runner`, remove the Startup fallback, and install/start the bundled official `svc.cmd` service. Verify the service account resolves every `.env` tool and GitHub returns online/idle after a service-only restart. | Service installation requires elevation. Stop and request it rather than adding another scheduler or silently relying on an interactive listener. |
| Orphaned OpenConsole installer lock appeared during recovery. | Unrelated installer state from Windows/app setup, not a GitHub runner issue. | Bootstrap does not touch OpenConsole; it reports only runner prerequisites. | Clear the installer lock separately if it blocks OS/package maintenance. Do not couple it to runner registration. |
| `pwsh: command not found` in `Ensure Rust toolchain` and cleanup. | Clean runner PATH had no PowerShell 7. | Install official PowerShell zip and prepend its directory in `actions-runner/.env`. | If PowerShell download is blocked, install PowerShell 7 manually and add its directory to `.env`. |
| `rustup.exe not found`; no `cargo` or `rustc`. | Clean runner account had no Rust toolchain. | Install rustup stable MSVC, preinstall pinned `1.95.0`, clippy/rustfmt, and Windows MSVC targets; prepend `%USERPROFILE%\.cargo\bin`. | If network blocks rustup, mirror the official rustup installer and toolchain artifacts internally. |
| ARM64 matrix job loses `rustup.exe` after x64 passes. | `Swatinem/rust-cache` post-job cleanup can remove `%USERPROFILE%\.cargo\bin` between sequential matrix jobs. | Bootstrap copies Rust proxies to `%USERPROFILE%\tools\rustup-proxies` and prepends that path before Cargo in the runner `.env`. | Restore proxies with `rustup-init` only if both the Cargo and durable proxy directories are absent. |
| `node` missing in `Prepare compile stubs`. | Clean runner PATH had no Node.js. | Install official Node LTS zip, prepend it in `.env`, install `pnpm`, and add shims so PowerShell does not pick blocked `.ps1` launchers. | If Node download is blocked, install supported Node LTS manually and add it to `.env`. |
| `link.exe` missing in Rust lib tests. | Visual Studio Build Tools and/or Windows SDK were absent. | Bootstrap detects `vswhere`, `vcvarsall.bat`, and Windows SDK inventory; with `-InstallVisualStudioBuildTools` in an elevated shell it installs Build Tools C++ workload, ARM64 tools, and Windows SDK, then writes MSVC `PATH`, `INCLUDE`, and `LIB` into runner `.env`. | If elevation/UAC is unavailable, install Visual Studio Build Tools manually with C++ tools and Windows SDK, then re-run bootstrap. |

## Verification Checklist

Run this after bootstrap:

```powershell
.\scripts\windows-runner-bootstrap.ps1 -VerifyOnly
```

The output must prove command resolution from the exact runner `.env` PATH:

- `pwsh -> %USERPROFILE%\tools\PowerShell\<version>\pwsh.exe`
- `rustup`, `cargo`, `rustc -> %USERPROFILE%\tools\rustup-proxies\*.exe`
- `node -> %USERPROFILE%\tools\Node\<version>\node.exe`
- `npm`, `corepack`, `pnpm -> %USERPROFILE%\tools\runner-shims\*.cmd`
- `git ->` installed Git command path
- MSVC inventory has `VisualStudioBuildTools: true` and a `vcvarsall.bat`
- Windows SDK inventory has `WindowsSdk: true`
- WebView2 inventory reports an installed Evergreen runtime when release builds need it
- the official runner service is installed and running under the intended runner account; no Startup `run.cmd` fallback remains
- GitHub reports the registered runner online and idle after restarting only that service

Then dispatch the non-secret `tests.yml` workflow and confirm `rust (windows)` reaches `Rust lib tests`. A fully restored builder must complete `rust (windows)` successfully before release workflows are trusted.

## Secret Handling

Registration tokens, signing keys, package tokens, and Tauri signing passwords stay outside the repository and outside this script. Pass runner registration through `-RegistrationTokenCommand` so the token can be fetched from an ephemeral external source at runtime. The script never writes the token to `.env`, docs, or logs.
