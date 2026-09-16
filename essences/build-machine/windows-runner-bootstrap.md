# Windows Runner Bootstrap

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

